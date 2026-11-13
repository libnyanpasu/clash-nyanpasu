# Remote access, CLI, agent skill and MCP: design

Status: **proposed**. Nothing here is implemented. The maintainer's decisions
are recorded in [§11](#11-decisions).

This design covers four connected pieces:

1. HTTP access control: a persisted switch (off by default) and three exposure
   strategies.
2. Command capabilities: what each caller may call, over which transport.
3. `nyanpasu-cli`, shipped with the app, talking to a running shell over HTTP or a
   local socket (Unix domain socket, Windows named pipe).
4. An agent skill and an MCP server built on the CLI.

It refines phase E of the
[platform extraction plan](../../design/application-platform-extraction.md#next-stage)
and follows [Unified RPC](../../development/rpc.md) and
[Architecture and ownership](../../development/architecture.md). The desktop
shell comes first; `nyanpasu-server` reuses the same pieces in E4.

## 1. Current state (`main @ 9cbb534c7`)

| Fact                                                                                                                                                                                                     | Source                                                         |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| The HTTP server is a "debug HTTP" runtime toggle. It is not persisted and starts off.                                                                                                                    | `nyanpasu-core/src/client/debug_http.rs`; `T/ipc.rs:2569-2582` |
| It binds `127.0.0.1:0` (random port) only.                                                                                                                                                               | `T/server/debug_http.rs:143`                                   |
| Authentication is a per-start UUID passed once as `?access_token=` on `GET /`, exchanged for an `HttpOnly; SameSite=Strict` cookie. Host must equal the bound authority; a present Origin must match it. | `T/server/debug_http.rs:145,271-330`                           |
| A second cookie, `nyanpasu_http_session`, identifies the `RpcOwner` (log sessions). It is not an authenticator.                                                                                          | `T/unified_rpc.rs:225-246`                                     |
| The server refuses to start without frontend assets; there is no API-only mode.                                                                                                                          | `T/server/debug_http.rs:141`                                   |
| Routes: `/bridge/rpc`, `/bridge/events` (SSE), `/bridge/logs/archive`, `/bridge/connection-details` (SSE).                                                                                               | `T/unified_rpc.rs:418-424`                                     |
| Exposure is a single boolean per command, `rpc(http)`: 104 of 164 declarations. There is no notion of _who_ may call a command, only _whether_ the listener accepted the request.                        | `nyanpasu-macro/src/unified_command.rs:6-29`; `T/ipc.rs`       |
| Every dispatched call passes `dispatch_audited` (HTTP at INFO). Authentication rejections are outside the audit boundary.                                                                                | `T/unified_rpc.rs:142`; [rpc.md](../../development/rpc.md)     |
| `nyanpasu-local-channel` provides framed, same-user-only sockets (pipe DACL, Unix socket permissions), but only as a one-shot parent/child server.                                                       | `nyanpasu-local-channel/src/lib.rs`, `tokio.rs:36-59`          |
| Precedent for agent skills: core archives ship skills, and `CoreAgentSkill` reports each skill's folder for agents to link.                                                                              | `nyanpasu-core/src/client/agent_skills.rs`; `clash_core.rs:58` |
| TypeScript bindings are generated from the specta type collection by `specta_export::tests::export_typescript_bindings`.                                                                                 | `T/specta_export.rs`                                           |
| Sidecars ship through Tauri `externalBin`.                                                                                                                                                               | `backend/tauri/tauri.conf.json:43-51`                          |

`T/` is `backend/tauri/src/`.

## 2. Goals and non-goals

Goals:

- HTTP remote access is **off by default**, persisted, and has three
  strategies:
  - `local_authenticated`: loopback only, authentication required;
  - `local_open`: loopback only, no authentication (Host/Origin checks stay);
  - `lan_authenticated`: listens on the LAN over HTTPS only, authentication
    required for every request, including requests from loopback.
- The local socket is a **separate switch**, also off by default; enabling the
  CLI or MCP does not open HTTP.
- One command-capability model covers the WebUI, CLI over HTTP, CLI over the
  local socket and MCP.
- `nyanpasu-cli` ships with every desktop bundle and drives a running desktop
  app; later the same binary drives `nyanpasu-server`.
- MCP is a CLI subcommand, `nyanpasu-cli mcp`, over stdio.
- Every authenticated call is attributable to a principal in the audit log.

Non-goals for this stage:

- A multi-user account system with passwords and roles, or pairing codes.
- Exposing the app to the internet. LAN is the widest supported exposure;
  anything beyond is the user's reverse proxy and their responsibility.
- An HTTP MCP endpoint for remote agents.
- Elevation, OS integration or shell capabilities over HTTP. These stay
  desktop-only, as today.

## 3. Settings model

A new persisted section of the application config, owned by `StateActor` like
any other setting:

```yaml
remote_access:
  http:
    enabled: false # master switch, default off
    strategy: local_authenticated # local_authenticated | local_open | lan_authenticated
    port: null # null until the first start picks a free port and persists it
    lan_bind: null # null: all interfaces; or a specific address
    tls: http # http | self_signed; lan_authenticated requires self_signed
    serve_webui: true # API-only when false
    local_open_scopes: [read] # what an anonymous loopback caller gets
  local_socket:
    enabled: false
```

Rules:

- `strategy` only matters while `enabled` is true. Turning the switch on keeps
  the previously chosen strategy; the first time it defaults to
  `local_authenticated`, which is today's behavior.
- **Port.** When the server first starts with `port: null`, it binds a random
  free port and persists it, so the URL, CLI contexts and pinned certificates
  stay valid across restarts. Later starts reuse the persisted port. If it is
  taken, the start is degraded and reported; the server does not silently move
  to another port. The user can change the port or reset it to `null`.
- **TLS.** The loopback strategies default to plain `http` and may opt into
  `self_signed`. `lan_authenticated` always uses `self_signed`; validation
  rejects `tls: http` with it before commit, and switching to LAN sets it.
- Changing `remote_access` follows the state rules: commit the config, then the
  HTTP server owner reconfigures its listener. A bind failure is a **degraded**
  result of the commit, not a rollback, and the status reports it.
- `remote_access` itself can only be patched through Tauri IPC and the local
  socket, never over HTTP (see [§5.3](#53-hard-rules)). This prevents a LAN
  caller from widening its own exposure or locking the owner out.
- The development frontend proxy (`Frontend::Dev`) stays a debug-only feature
  and is not part of these settings.

## 4. Authentication

### 4.1 Principals

Every request resolves to exactly one principal before dispatch:

| Principal          | How it is established                                      | Typical caller                     |
| ------------------ | ---------------------------------------------------------- | ---------------------------------- |
| `Desktop(window)`  | Tauri IPC, process boundary                                | Desktop webview                    |
| `LocalSocket(uid)` | Accepted connection on the same-user socket                | `nyanpasu-cli`, `nyanpasu-cli mcp` |
| `Token(id)`        | `Authorization: Bearer <token>` over HTTP                  | CLI/MCP/scripts over HTTP          |
| `Session(id)`      | Browser session cookie, created by a launch link or login  | WebUI in a browser                 |
| `Anonymous`        | Only in `local_open`, loopback request without credentials | Local scripts, local browser       |

`RpcOwner` (resource ownership such as log sessions) is derived from the
principal and, for sessions, the session id. It is never supplied by the client.
The current separate `nyanpasu_http_session` cookie becomes the session itself.

### 4.2 Access tokens

- Long-lived, named, created by the owner from the desktop GUI or
  `nyanpasu-cli remote token create` over the local socket.
- Format `npk_<id>_<secret>`: a public id for listing, revoking and audit, and a
  256-bit random secret. Only a SHA-256 hash of the secret is kept (high
  entropy makes a slow hash unnecessary); the secret is shown once.
- Each token carries a scope set ([§5](#5-command-capabilities)) and an optional
  expiry. Revoking a token ends every session created from it.
- Storage follows [§4.6](#46-secrets-at-rest).

### 4.3 Browser login

A browser cannot send a bearer header on navigation, so the WebUI uses a
session cookie (`HttpOnly; SameSite=Strict; Path=/`, plus `Secure` under TLS).
There are exactly two ways to obtain one:

1. **Launch link** (all strategies): the desktop "Open WebUI" action mints a
   one-time code, valid for 60 seconds, and opens `/?code=…`. This replaces
   today's per-start `access_token`, which is reusable for the whole run.
2. **Login with a token** (all authenticated strategies, the only way from
   another device on the LAN): a minimal login page accepts an access token
   and exchanges it for a session bound to that token's scopes.

Sessions are held by the remote-access owner with an idle timeout and are
revoked with their token.

### 4.4 Request checks per strategy

| Check                                | `local_open`                | `local_authenticated` | `lan_authenticated`                                        |
| ------------------------------------ | --------------------------- | --------------------- | ---------------------------------------------------------- |
| Bind                                 | `127.0.0.1`, `::1`          | `127.0.0.1`, `::1`    | `lan_bind` or all interfaces                               |
| Scheme                               | HTTP, or HTTPS if opted in  | Same                  | HTTPS only                                                 |
| Host header                          | Must be the bound authority | Same                  | Any address the listener is bound to, or a configured name |
| Origin (when present)                | Must equal the Host origin  | Same                  | Same                                                       |
| Credential                           | Optional                    | Required              | Required, also from loopback                               |
| JSON `content-type` on `/bridge/rpc` | Required                    | Required              | Required                                                   |
| Failed-auth rate limit per peer      | n/a                         | Yes                   | Yes                                                        |

Notes:

- `lan_authenticated` never trusts the peer address. A reverse proxy makes
  every request look like loopback, so loopback is not an exemption.
- Requiring `application/json` forces a CORS preflight for cross-origin
  browser requests; the server answers no preflight, so a malicious page cannot
  post to `local_open`. Origin checks cover the rest.
- `local_open` trusts **every local process of every OS user** on the
  machine, because loopback TCP is not user-scoped. The settings UI states
  this, and `local_open_scopes` defaults to `[read]`. Users who want an
  unauthenticated CLI should use the local socket, which is same-user only.
- Credential comparison is constant-time; 401 responses carry no detail.
- Authentication failures are logged at WARN on a separate target
  (`nyanpasu::remote_access`) with the peer address and the token id if one was
  parsed, never the secret. Successful calls go through `dispatch_audited` as
  today, with the principal as the caller.

### 4.5 TLS

- The certificate is self-signed, generated on first use, and covers the
  loopback addresses, the configured `lan_bind` (or the host's addresses) and
  the host name. It is regenerated only on explicit reset or when it expires.
- The desktop settings show its SHA-256 fingerprint. The CLI pins it: the
  fingerprint is given with `--fingerprint` when adding a context, or accepted
  on first use after the CLI prints it, then stored in the context. A changed
  certificate is a hard error until the user re-pins.
- Browsers show a warning once per certificate; the login page explains how to
  compare the fingerprint.

### 4.6 Secrets at rest

The remote-access owner keeps token hashes, session metadata and the TLS
private key in the backend through an injected storage port. The port reports
whether its medium is protected:

- **Protected** (the store is readable only by the owning OS user: a `0600`
  file in a `0700` directory on Unix, an owner-only ACL on Windows): records are
  stored as they are.
- **Not protected** (for example a shared or relaxed directory, or a medium
  without owner-only access): records are encrypted with a key held in the OS
  credential store (Keychain, Credential Manager, Secret Service). If no
  credential store is available, the owner refuses to persist secrets and
  reports it instead of writing them in the clear.

These records are excluded from config backups and exports; a restored
installation creates new tokens and a new certificate.

## 5. Command capabilities

### 5.1 Two independent axes

- **Exposure (per target):** whether a transport offers the command at all. This
  is the planned E1a per-target opt-in: `rpc(http)` (WebUI/HTTP) and
  `rpc(local)` (local socket). Tauri IPC exposes everything. A command not
  exposed on a target returns a structured `unsupported` error from the
  registry.
- **Scope (per principal):** what a principal may do among the exposed
  commands. Each command declares exactly one scope; each principal holds a
  scope set.

A call succeeds only if the command is exposed on the transport **and** its
scope is in the principal's set. The CLI over HTTP therefore sees the HTTP
allowlist and the CLI over the socket sees the local allowlist. "CLI" is not a
transport and gets no allowlist of its own.

### 5.2 Scopes

| Scope            | Covers                                                                                                | Classification          |
| ---------------- | ----------------------------------------------------------------------------------------------------- | ----------------------- |
| `read`           | Queries without secrets: status, proxies, profiles, rules, logs, traffic, connections, settings reads | Default for every query |
| `sensitive_read` | Queries that return secrets: Clash `secret`, subscription URLs, profile contents, raw runtime config  | Declared per query      |
| `proxy`          | Select proxy, latency tests, switch mode, close connections                                           | Declared per mutation   |
| `profile`        | Profile and subscription CRUD, update subscriptions, chains, runtime patches                          | Declared per mutation   |
| `settings`       | Application and Clash config patches that are not system or remote-access settings                    | Declared per mutation   |
| `system`         | Core start/stop/restart/switch, system proxy, TUN, transparent proxy                                  | Declared per mutation   |
| `remote_admin`   | `remote_access` settings, tokens, sessions                                                            | Declared per mutation   |

Declaration, extending the macro:

```rust
#[nyanpasu_macro::rpc(http, local, scope = proxy)]
pub async fn select_proxy(/* … */) -> Result<()> { /* … */ }

#[nyanpasu_macro::rpc(http, local, scope = sensitive_read)]
pub async fn get_profile_content(/* … */) -> Result<String> { /* … */ }
```

- Queries default to `read`; a query that returns a secret must declare
  `sensitive_read`, and the review of each `rpc(http)` query includes this
  check. A mutation without `scope` fails to compile; the macro source tests
  already check every declaration and gain this check.
- A token holds `system` only when it was created with it; it is never part of
  a default scope set.
- The scope list is the protocol's, not the CLI's: it lives in
  `nyanpasu-rpc-protocol` with the command metadata (E1a).

### 5.3 Hard rules

- `remote_admin` is never exposed over HTTP. Remote access is managed from the
  desktop GUI or the local socket only.
- Elevation, OS integration and shell capabilities keep their existing
  restrictions; a scope never grants a capability the shell does not provide.
- `local_open` anonymous callers never hold `sensitive_read`, `system` or
  `remote_admin`, whatever `local_open_scopes` says; validation rejects such a
  patch.
- `LocalSocket(uid)` holds all scopes: it is the same OS user who owns the app.

### 5.4 Capability discovery

A new query, `rpc_describe` (working name), exposed on every target, returns:

```json
{
  "protocol": 1,
  "server": { "kind": "desktop", "version": "2.0.0" },
  "transport": "http",
  "principal": { "kind": "token", "id": "a1b2", "name": "laptop-cli" },
  "scopes": ["read", "proxy"],
  "commands": [
    {
      "name": "select_proxy",
      "kind": "mutation",
      "scope": "proxy",
      "allowed": true
    }
  ]
}
```

The CLI uses it to check protocol compatibility, hide or reject subcommands the
principal cannot use with a clear message, and select MCP tools. It reports
metadata only; payload schemas come from the generated schema file
([§9.2](#92-mcp)).

## 6. Transports

### 6.1 HTTP

- API-only mode: the router starts without frontend assets when
  `serve_webui: false` or when no assets exist (the headless server). This is
  the existing blocker at `T/server/debug_http.rs:141`.
- The bearer header is accepted on `/bridge/*` only; static assets accept only
  sessions.
- The HTTP server owner (`HttpServerActor`, renamed from debug HTTP) receives
  the whole `remote_access.http` value instead of `SetEnabled(bool)`, and
  restarts the listener when bind-relevant fields change. Graceful-drain
  behavior stays as it is.
- The authenticator is a port consumed by the HTTP transport and implemented by
  the remote-access owner, so the transport has no token or session state of
  its own.

### 6.2 Local socket

| Platform | Endpoint                                                                        | Protection                                               |
| -------- | ------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Linux    | `$XDG_RUNTIME_DIR/clash-nyanpasu/rpc.sock`, falling back to the app runtime dir | Directory `0700`, socket `0600`, `SO_PEERCRED` uid check |
| macOS    | App data dir `run/rpc.sock`                                                     | Same, `getpeereid` uid check                             |
| Windows  | `\\.\pipe\clash-nyanpasu-rpc-<user SID hash>`                                   | DACL for the current user only, client token check       |

- Long-lived and multi-client, unlike `OneShotServer`. Add a listener to
  `nyanpasu-local-channel` and reuse its framing and security setup, as the
  extraction plan suggests.
- Wire: length-prefixed JSON frames with the same envelope as `/bridge/rpc`:
  `{ "id", "method", "params" }` → `{ "id", "result" }` or
  `{ "id", "error": RpcError }`, plus `subscribe`/`unsubscribe` frames for
  events and log streams. One connection is one `RpcOwner`; closing it releases
  its subscriptions and owned resources.
- Enters `dispatch_audited` at INFO, as the extraction plan already states.
- The endpoint path is derived from `nyanpasu-paths`, so the CLI finds it
  without a discovery file and without a running app to ask.

## 7. Ownership and packages

| Piece                                                                                                | Kind           | Home                                                                                                                                            |
| ---------------------------------------------------------------------------------------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `remote_access` settings schema, validation                                                          | Pure           | `nyanpasu-config`                                                                                                                               |
| `RemoteAccessActor` / `RemoteAccessClient`: tokens, sessions, launch codes, rate limits, certificate | Actor          | Core package family; exposed through `NyanpasuClient` domain operations (`create_access_token`, `revoke_access_token`, `list_access_tokens`, …) |
| Secret storage and its protection report                                                             | Port + adapter | Port in core, implementation injected by the shell (storage backend and OS credential store)                                                    |
| Scope and exposure metadata                                                                          | Protocol       | `nyanpasu-rpc-protocol` (E1a)                                                                                                                   |
| Authorization check (exposure × scope)                                                               | Registry       | `nyanpasu-rpc` (E2), before `dispatch_audited` calls the handler                                                                                |
| HTTP listener, bearer/session extraction, Host/Origin, TLS                                           | Transport      | HTTP transport (moves out of `T/server/` in E3)                                                                                                 |
| Local-socket listener and client                                                                     | Transport      | Local-socket transport over `nyanpasu-local-channel`                                                                                            |
| Generated command schema file                                                                        | Generated      | Exported next to the TypeScript bindings, embedded by the CLI                                                                                   |
| `nyanpasu-cli`, MCP subcommand                                                                       | Client         | `backend/nyanpasu-cli`, depends on protocol and transport clients only, never core                                                              |

No Tauri type enters any of these except the desktop composition root that
assembles them. The remote-access owner holds no reference to the HTTP server;
the server consumes the authenticator port, which avoids a reference cycle.

## 8. `nyanpasu-cli`

### 8.1 Connection

- `--transport auto|socket|http`. `auto` tries the local socket, then the
  current HTTP context.
- Contexts, kubectl-style, in the CLI's config file (`cli.toml` under the
  user config dir): name, URL, token reference, pinned TLS fingerprint.
  `nyanpasu-cli context add|use|list|remove`.
- Tokens are stored in the OS keychain when available, otherwise in a `0600`
  file. `NYANPASU_URL` and `NYANPASU_TOKEN` override for scripts; a token is
  never accepted as a plain argument value, to keep it out of shell history.
- On connect the CLI calls `rpc_describe` and refuses an incompatible protocol
  with a message naming both versions.

### 8.2 Commands

```text
nyanpasu-cli status
nyanpasu-cli core      start | stop | restart | switch <core> | version
nyanpasu-cli mode      get | set <rule|global|direct>
nyanpasu-cli proxy     groups | list <group> | select <group> <proxy> | test <group|proxy>
nyanpasu-cli profile   list | use <id…> | update [<id>|--all] | import <url> | show <id>
nyanpasu-cli sysproxy  status | on | off
nyanpasu-cli tun       status | on | off
nyanpasu-cli conn      list | close <id>|--all
nyanpasu-cli logs      tail [-f] [--level]
nyanpasu-cli traffic   [-f]
nyanpasu-cli config    get [<path>] | patch <json>
nyanpasu-cli remote    status | http on|off|strategy <s> | socket on|off
                       token create|list|revoke | session list|revoke   # local socket only
nyanpasu-cli methods                 # rpc_describe, human readable
nyanpasu-cli call <method> [<json>]  # raw call of any allowed command
nyanpasu-cli context   add | use | list | remove
nyanpasu-cli mcp       [--scope <s>…]
nyanpasu-cli skill     path | install [--agent <name>]
nyanpasu-cli completion <shell>
```

### 8.3 Output and exit codes

- Human-readable tables by default; `--json` prints the RPC result unchanged,
  stable for scripts and agents. Streams print one JSON object per line.
- Structured outcomes are preserved: a degraded mutation prints its degradation
  and exits with its own code, never success.

| Exit | Meaning                                                   |
| ---- | --------------------------------------------------------- |
| 0    | Success                                                   |
| 1    | Application error (`RpcError` from the domain)            |
| 2    | Usage error                                               |
| 3    | Not reachable: app not running, socket disabled, HTTP off |
| 4    | Not authenticated or scope denied                         |
| 5    | Unsupported on this target or shell                       |
| 6    | Applied with degradation                                  |

### 8.4 v0 before the typed protocol

The extraction plan puts the CLI after D2 and E1b, because the complete request
and response DTOs cannot move into the protocol before then. The CLI does not
wait for that:

- **CLI v0** depends on command names, the error envelope and scope metadata
  (E1a), plus the generated schema file ([§9.2](#92-mcp)). Curated subcommands
  build their `params` as JSON, validated against the schema, and render results
  from `serde_json::Value`; `call` covers everything else.
- **CLI v1** switches curated subcommands to protocol DTOs once E1b lands.

The schema file keeps v0's hand-built JSON honest: a renamed field fails
validation in the CLI integration tests (§10) instead of at the user.

### 8.5 Shipping

| Platform | Install location                                        | PATH                                                                                                           |
| -------- | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Windows  | Next to the app through `externalBin`                   | NSIS/MSI option "Add the CLI to PATH"; Scoop `bin` shim                                                        |
| macOS    | `Contents/MacOS/nyanpasu-cli`                           | GUI action "Install command-line tool" links it into `~/.local/bin` or prints the command for `/usr/local/bin` |
| Linux    | `/usr/bin/nyanpasu-cli` in deb/rpm; inside the AppImage | Packages: already on PATH; AppImage: GUI action as on macOS                                                    |

The CLI version equals the app version. It is a small binary (no core, no
script engines), which the boundary gate enforces.

## 9. Agent skill and MCP

### 9.1 Skill

- `SKILL.md` plus short reference files, shipped as bundle resources under
  `skills/nyanpasu-cli/`, following the `CoreAgentSkill` precedent: the app
  reports the folder; agents link it so updates refresh it in place.
- `nyanpasu-cli skill path` prints the folder; `skill install --agent <name>`
  links it into that agent's skill directory.
- Content: always use `--json`; start with `status` and `methods`; treat
  queries as safe and confirm mutations with the user; map exit codes; never
  create tokens or change `remote` settings unless asked.
- The skill is versioned with the CLI it describes; tests check that every
  command the skill names exists.

### 9.2 MCP

- `nyanpasu-cli mcp` is a stdio MCP server that reuses the CLI's connection
  logic, contexts and credentials.
- **Read-only by default:** it registers tools for `read` commands only.
  `--scope proxy --scope profile …` adds more, always intersected with the
  principal's scopes from `rpc_describe`. `remote_admin` is never offered.
- Curated tools (status, list/select proxy, switch mode, update profiles, tail
  logs) have handwritten descriptions. A generic `call_method` tool is opt-in
  (`--raw`).
- **Input and output schemas are generated from the start.** The specta type
  collection that produces the TypeScript bindings also exports a JSON Schema
  file for every command exposed on `http` or `local`, in the same export
  workflow and with the same "regenerate, never hand-edit" rule. The CLI embeds
  the file, so it needs no core dependency and no running app to list tools. A
  test fails when the committed file is stale. Whether specta's own JSON Schema
  exporter is usable at the pinned 2.0 release candidate is checked first;
  otherwise a small exporter walks the collected types.
- Tool annotations: `readOnlyHint` for queries, `destructiveHint` for
  mutations that drop connections or stop the core.
- Each MCP process is one principal and one `RpcOwner`; tool calls are audited
  like any other call.

## 10. Verification

- Remote-access owner: tokens, sessions, launch codes, expiry, revocation and
  rate limits through its typed client with a fake clock and fake storage.
- Secrets at rest: a protected fake store receives plain records; an
  unprotected one receives encrypted records; no credential store refuses to
  persist; backups contain none of them.
- Settings: validation (`lan_authenticated` requires `self_signed`, `local_open`
  scope cap) as pure tests; first-start port selection persists the port; a
  taken persisted port degrades instead of moving; commit-then-reconfigure with
  a fake listener.
- Registry: exposure × scope matrix for every principal kind, including
  `remote_admin` rejected over HTTP and `unsupported` for unexposed commands.
- Macro: a mutation without `scope` fails to compile.
- HTTP: each strategy's checks from [§4.4](#44-request-checks-per-strategy),
  DNS-rebinding Host values, cross-origin and `text/plain` posts, bearer on
  static routes rejected, loopback not exempt under LAN, plain HTTP refused
  under LAN.
- Local socket: second OS user rejected (Unix CI), pipe DACL (Windows CI),
  owner release on disconnect.
- CLI: integration tests against a real server assembled with core test
  support, on both transports; certificate pinning and mismatch; exit codes;
  `--json` stability snapshots; curated params validated against the schema.
- MCP: tool list per scope set; read-only default; schema file freshness.
- Boundary gate: the CLI links no core crate; no Tauri dependency outside the
  shell.

## 11. Decisions

| #   | Question                      | Decision                                                                                                   |
| --- | ----------------------------- | ---------------------------------------------------------------------------------------------------------- |
| 1   | How a browser logs in         | Launch link and token login only; no pairing code, no password                                             |
| 2   | TLS                           | Self-signed certificate; the loopback strategies default to plain HTTP; `lan_authenticated` requires HTTPS |
| 3   | Port                          | The first start picks a random free port and persists it                                                   |
| 4   | Scope granularity             | The scopes in §5.2, including a separate `sensitive_read`                                                  |
| 5   | Binary name                   | `nyanpasu-cli`                                                                                             |
| 6   | CLI before the typed protocol | Ship v0 first                                                                                              |
| 7   | MCP schemas                   | Generate them from the start; no handwritten interim                                                       |
| 8   | Where secrets are stored      | In the backend; encrypted unless the storage is protected                                                  |
| 9   | Desktop or server first       | Desktop first                                                                                              |
| 10  | HTTP MCP endpoint             | Not needed for now                                                                                         |
| —   | Local socket switch           | Separate from HTTP, off by default                                                                         |
| —   | MCP form                      | `nyanpasu-cli mcp` over stdio                                                                              |
| —   | The three strategies          | `lan_authenticated`, `local_authenticated`, `local_open`                                                   |

## 12. Order

Each line is one reviewable PR or a short stack; it fits into phase E without
reordering the existing steps.

| Step | Change                                                                                                                                                   | Depends on                      |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| R1   | `remote_access` settings, persisted switch and port, rename debug HTTP; `local_authenticated` and `local_open`; launch codes replace the per-start token | —                               |
| R2   | Remote-access owner: tokens, sessions, token login, bearer auth, rate limits, auth audit, secrets at rest; API-only HTTP                                 | R1                              |
| R3   | Self-signed TLS and `lan_authenticated`                                                                                                                  | R2                              |
| R4   | Scope metadata and `rpc(local)` in the macro, registry authorization, `rpc_describe`, generated schema file                                              | E1a                             |
| R5   | Local-socket listener and transport, separate switch                                                                                                     | R4, E2 for the neutral registry |
| R6   | `nyanpasu-cli` v0 (both transports, contexts, pinning, exit codes), shipping and PATH integration                                                        | R2, R4, R5                      |
| R7   | Agent skill and `nyanpasu-cli mcp`                                                                                                                       | R6                              |
| R8   | CLI v1 on protocol DTOs                                                                                                                                  | E1b                             |
