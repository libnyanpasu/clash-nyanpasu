# Remote access, CLI, agent skill and MCP: design

Status: **in progress**. Layers 01–03 are implemented; the remaining layers are proposed.
The maintainer's decisions are recorded in [§11](#11-decisions) and the delivery
state in [§12](#12-order).

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

## 1. Current state (`main @ b35722b58`)

Phase E of the platform extraction has since delivered the neutral RPC layer this design
builds on: `nyanpasu-rpc-protocol` (command catalog, owners, frames, typed operations),
`nyanpasu-rpc` (registry, `dispatch_audited`, HTTP and local-socket transports),
`nyanpasu-single-instance::endpoint`, and the headless `nyanpasu-server`. The CLI
(E4c) and the broader typed protocol are not delivered.

| Fact                                                                                                                                                                                                                                                                                          | Source                                                                                  |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| The HTTP listener is owned by `HttpServerActor` (desktop) or `LoopbackListener` (headless server), both with the same `local_session` middleware. Before layer 01 it was an unpersisted "debug HTTP" toggle.                                                                                  | `nyanpasu-rpc/src/http/`                                                                |
| It binds `127.0.0.1` only. Authentication is a per-start UUID passed once as `?access_token=` on `GET /`, exchanged for an `HttpOnly; SameSite=Strict` cookie; Host must equal the bound authority.                                                                                           | `nyanpasu-rpc/src/http/mod.rs`                                                          |
| `nyanpasu_http_session` is parsed as any client-supplied UUID and used as the `RpcOwner`; without it a request gets a fresh random owner. It is not an authenticator and has no server-side registry.                                                                                         | `nyanpasu-rpc/src/http/routes.rs`                                                       |
| Routes: `/bridge/rpc`, `/bridge/events` (SSE), `/bridge/logs/archive`, `/bridge/connection-details` (SSE), `/bridge/icons/{*path}`. The icon route is a direct route that needs a scope like the others.                                                                                      | `nyanpasu-rpc/src/http/routes.rs`                                                       |
| Exposure was originally `rpc(http)` (100 commands) and `rpc(local)` (13 commands); the shared authorization slice adds `rpc_describe` (14 local commands). `command_catalog.rs` and the capability inventory generate `effect` (`Query`/`Mutation`) and `scope`, matching `specta_export.rs`. | `nyanpasu-macro/src/unified_command.rs`; `nyanpasu-rpc-protocol/src/command_catalog.rs` |
| Privilege-separated settings: only `hotkeys` and `enable_service_mode` have dedicated commands. System-proxy, core selection, TUN, the Clash secret and others still pass the general patch commands over HTTP.                                                                               | `nyanpasu-rpc/src/commands/runtime.rs`                                                  |
| The local socket exists (framing, negotiation, per-connection owners, five full-value event streams) and is enabled only by `NYANPASU_LOCAL_RPC=1` on the desktop, or always by `nyanpasu-server`.                                                                                            | `nyanpasu-rpc/src/local/`; `backend/tauri/src/setup/local_rpc.rs`                       |
| Every dispatched call passes `dispatch_audited`; authentication rejections are outside it.                                                                                                                                                                                                    | `nyanpasu-rpc/src/registry.rs`; [rpc.md](../../development/rpc.md)                      |
| Precedent for agent skills: core archives ship skills, and `CoreAgentSkill` reports each skill's folder.                                                                                                                                                                                      | `nyanpasu-core/src/client/agent_skills.rs`                                              |
| TypeScript bindings are generated by `specta_export::tests::export_typescript_bindings`; the command catalog and capability inventory by `nyanpasu-macro` ignored tests.                                                                                                                      | `backend/tauri/src/specta_export.rs`; [rpc.md](../../development/rpc.md)                |
| Sidecars ship through Tauri `externalBin`.                                                                                                                                                                                                                                                    | `backend/tauri/tauri.conf.json`                                                         |

`T/` below is `backend/tauri/src/`.

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

A new persisted section of the application config, owned by the application
config source like any other setting. **Layer 01 delivers
`remote_access.http.{enabled, strategy, port, serve_webui}` with the single
strategy `local_authenticated`**; the other keys and strategies join the schema in
the layer that implements their complete request, scope and transport checks, so a
stored value cannot name a strategy that does not exist yet:

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
  result of the commit (`http_server_unavailable`), not a rollback, and the status
  reports the fault. A single serial owner (`RemoteAccessClient`, an actor in
  `nyanpasu-core`) runs the whole operation: commit, listener configuration and the
  recording of an initially open port. The owner is the section's only writer, so
  two concurrent edits are applied in order and the port recorded for an earlier edit
  cannot overwrite a later one; a caller that leaves does not cancel the operation.
- `remote_access` itself can only be patched through Tauri IPC and the local
  socket through a dedicated `remote_admin` command, never over HTTP (see
  [§5.3](#53-hard-rules)); layer 01 delivers `set_remote_access` on Desktop only.
  General config patches reject this section during decoding, through the concrete
  `ApplicationPreferencesPatch` request contract, which has no `remote_access` field. This prevents a LAN caller from widening its own exposure or
  locking the owner out.
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

The principal identifies the authenticated caller for authorization and audit.
`RpcOwner` identifies a resource lifetime, such as a log-reading session. They
are separate: two connections with the same UID or two processes using the same
token share a principal but have different owners.

| Caller                          | Resource owner                                               | Lifetime                        |
| ------------------------------- | ------------------------------------------------------------ | ------------------------------- |
| Desktop                         | Invoking window                                              | Window lifetime                 |
| Local socket                    | Server instance plus fresh connection identity               | Connection lifetime             |
| Browser session                 | Server-issued browser session identity                       | Session lifetime                |
| HTTP bearer or anonymous client | Fresh server-issued resource session, bound to the principal | Explicit release or idle expiry |

Bearer and anonymous clients establish a resource session with
`POST /bridge/resource-session` before opening owned resources. The response sets
an unguessable `nyanpasu_http_session` cookie (`HttpOnly; SameSite=Strict;
Path=/`, plus `Secure` under TLS); subsequent requests reuse it. The server
resolves the cookie to its stored owner and verifies its principal binding.
A cookie never substitutes for bearer authentication, grants scopes, or lets
the client choose an owner label. Browser authentication uses its separate
access-session cookie. Resource-session allocation is bounded per principal
and per listener, including anonymous clients. Owned operations without a
valid resource session are rejected before allocating anything.

Each CLI/MCP process uses its own in-memory cookie jar, never a context-wide
persisted resource cookie. `DELETE /bridge/resource-session` releases its owned
resources and subscriptions on normal exit; an idle timeout handles abandoned
clients. Browser session expiry/revocation and socket disconnect do the same
cleanup for their owners. Successful late resource allocations are also released;
cleanup does not cancel owner-started business work. These HTTP endpoints are
transport session controls, with the admission rules in
[§5.5](#55-direct-http-routes-and-streams).

### 4.2 Access tokens

- Long-lived, named, created by the owner from the desktop GUI or
  `nyanpasu-cli remote token create` over the local socket.
- Format `npk_<id>_<secret>`: a public id for listing, revoking and audit, and a
  256-bit random secret. Only a SHA-256 hash of the secret is kept (high
  entropy makes a slow hash unnecessary); the secret is shown once.
- Each token carries a scope set ([§5](#5-command-capabilities)) and an optional
  expiry. Revocation or expiry invalidates its browser and resource sessions,
  releases their owned resources and ends active bearer/session streams. It
  prevents new admissions without cancelling business work already started.
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
revoked with their token. The browser session is both its authenticated principal
and the lifetime from which its resource owner is derived; bearer and anonymous
resource cookies do not log a browser in.

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
- The scope list is the protocol's, not the CLI's. Its single canonical definition is
  `nyanpasu_config::application::Scope`, because config validates
  `local_open_scopes`, core stores token scopes and the protocol's command metadata
  uses the same value; `nyanpasu-rpc-protocol` re-uses it and defines no second enum.

One scope per command requires a matching request boundary. General
`patch_app_config` and `patch_clash_config` accept ordinary preferences only.
Settings with another privilege class move to dedicated commands and domain
patch types; the general patch decoder rejects them, including mixed patches,
before handler admission. This applies on every transport, even for a caller
holding all scopes. In particular:

- `enable_system_proxy`, TUN and transparent-proxy settings, service mode and
  core selection belong to `system` commands, with separately reviewed target
  exposure and the existing shell/elevation restrictions.
- The `remote_access` section belongs only to `remote_admin` commands exposed
  on Desktop and Local.
- Runtime override patches must likewise separate proxy-mode operations from
  settings with a different scope; `config patch` and raw `call` cannot bypass
  these boundaries.

Rejected patches are atomic: `false`, empty collections and same-value writes
still count as requested fields, and allowed fields in a mixed patch are not
applied. Sensitive query responses also need a matching boundary: an existing
response containing subscription URLs or the Clash secret is `sensitive_read`
until a separate secret-free query is provided; it cannot default to `read`.

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

Every resolved principal may discover its own permissions and per-target command
availability without holding `read`. This discovery exception returns metadata only:
it grants no business-data access, never discloses another principal's information,
and does not admit an unresolved or invalid credential.

The CLI uses it to check protocol compatibility, hide or reject subcommands the
principal cannot use with a clear message, and select MCP tools. It reports
metadata only; payload schemas come from the generated schema file
([§9.2](#92-mcp)).

### 5.5 Direct HTTP routes and streams

Command metadata alone does not protect routes outside `/bridge/rpc`. The HTTP
adapter uses the same authorization policy and audit kernel before executing a
direct handler, generating an archive or acquiring a stream receiver:

| Entry                                      | Required scope and admission                                                                                                                                                 |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/bridge/logs/archive`                     | `sensitive_read` for the diagnostic archive (the redactor leaves opaque URL host/path tokens intact, so unprivileged `read` is excluded unless a token-safe export is added) |
| `/bridge/connection-details`               | `read` before subscribing                                                                                                                                                    |
| `/bridge/events?name=…`                    | Explicit per-event exposure and scope metadata; events without reviewed metadata are unavailable                                                                             |
| `/bridge/events` without a name            | Deliver only events exposed to HTTP whose required scope the principal holds                                                                                                 |
| `POST` / `DELETE /bridge/resource-session` | Resolve the principal under the listener strategy and enforce principal binding on release; neither operation grants scopes or returns application data                      |

The event review covers payloads and reset/resync delivery, including generic
storage values, rather than inheriting `read` for every event. A rejected named
subscription acquires no receiver; an unfiltered subscription never forwards
unauthorized payloads. Scope sets are independent: a token holding only `proxy`
cannot obtain connection details, logs or `read` events.

Streams are registered against their principal and resource lifetime. The
remote-access owner exposes revocation/expiry notifications through the injected
port; adapters stop delivery and release receivers when that lifetime ends,
including existing bearer SSE connections. This stops observation, not admitted
business work. Admission denials and stream termination retain audit records
without credentials or payloads.

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

| Piece                                                                                                                 | Kind                 | Home                                                                                                                                                  |
| --------------------------------------------------------------------------------------------------------------------- | -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `remote_access` settings schema, validation                                                                           | Pure                 | `nyanpasu-config`                                                                                                                                     |
| `RemoteAccessActor` / `RemoteAccessClient`: tokens, browser/resource sessions, launch codes, rate limits, certificate | Actor                | Core package family; exposed through `NyanpasuClient` domain operations (`create_access_token`, `revoke_access_token`, `list_access_tokens`, …)       |
| Secret storage and its protection report                                                                              | Port + adapter       | Port in core, implementation injected by the shell (storage backend and OS credential store)                                                          |
| Scope and exposure metadata                                                                                           | Protocol             | `nyanpasu-rpc-protocol` (E1a)                                                                                                                         |
| Authorization check (exposure × scope)                                                                                | Registry + transport | Shared policy in `nyanpasu-rpc` (E2), used inside the audited admission boundary by commands, direct routes and stream subscriptions before execution |
| HTTP listener, bearer/session extraction, Host/Origin, TLS                                                            | Transport            | HTTP transport (moves out of `T/server/` in E3)                                                                                                       |
| Local-socket listener and client                                                                                      | Transport            | Local-socket transport over `nyanpasu-local-channel`                                                                                                  |
| Generated command schema file                                                                                         | Generated            | Exported next to the TypeScript bindings, embedded by the CLI                                                                                         |
| `nyanpasu-cli`, MCP subcommand                                                                                        | Client               | `backend/nyanpasu-cli`, depends on protocol and transport clients only, never core                                                                    |

No Tauri type enters any of these except the desktop composition root that
assembles them. The remote-access owner holds no reference to the HTTP server;
the server consumes the authenticator port, which avoids a reference cycle.

## 8. `nyanpasu-cli`

### 8.1 Connection

- Contexts, kubectl-style, in the CLI's config file (`cli.toml` under the
  user config dir): name, target kind (`local_socket` or `http`), and either
  the local endpoint or the HTTP URL, token reference and pinned TLS fingerprint.
  `nyanpasu-cli context add|use|list|remove`; `--context <name>` selects one
  invocation without changing the current context.
- Resolve the target before connecting: an explicit `--context` selects that
  context, otherwise `NYANPASU_URL` selects an HTTP target, otherwise use the
  current context. With none configured, use the default local socket.
- `--transport auto|socket|http` selects a transport for that target only.
  `auto` uses the context's target kind. An incompatible explicit transport is
  a usage error. A failed connection never falls back to another context or
  instance: a remote HTTP context remains remote even when a local socket is
  available, and a missing local socket never redirects to an HTTP context.
- Tokens are stored in the OS keychain when available, otherwise in a `0600`
  file. `NYANPASU_TOKEN` overrides the selected HTTP target's credential; it
  does not change the target. A token is never accepted as a plain argument
  value, to keep it out of shell history.
- On connect the CLI calls `rpc_describe` and refuses an incompatible protocol
  with a message naming both versions. HTTP then establishes its own resource
  session as described in [§4.1](#41-principals).

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
- Each MCP process uses the selected target's principal and has its own
  `RpcOwner`: a dedicated socket connection or HTTP resource session. Processes
  sharing a UID or token remain separate resource owners; tool calls retain both
  principal attribution and resource-owner correlation in audit.

## 10. Verification

- Remote-access owner: tokens, sessions, launch codes, expiry, revocation and
  rate limits through its typed client with a fake clock and fake storage.
- Resource ownership: concurrent socket clients with the same UID and HTTP
  clients with the same token have distinct owners; anonymous clients also
  have independent resource sessions. Cookies bound to another principal and
  missing/expired resource sessions are rejected before allocation; session
  capacity limits reject excess allocations. Explicit release, idle expiry
  and disconnect clean up only that owner's resources,
  including successful late allocations. Token revocation/expiry closes all
  of its owners and active SSE streams without cancelling admitted mutations.
- Secrets at rest: a protected fake store receives plain records; an
  unprotected one receives encrypted records; no credential store refuses to
  persist; backups contain none of them.
- Settings: validation (`lan_authenticated` requires `self_signed`, `local_open`
  scope cap) as pure tests; first-start port selection persists the port; a
  taken persisted port degrades instead of moving; commit-then-reconfigure with
  a fake listener.
- Registry: exposure × scope matrix for every principal kind, including
  `remote_admin` rejected over HTTP and `unsupported` for unexposed commands.
- Patch boundaries: settings-only tokens cannot write system fields through
  either general patch; no general patch accepts `remote_access`, even over
  Desktop/Local with all scopes. Mixed, false and same-value requests reject
  atomically before admission. Dedicated commands enforce their own scope and
  target. Queries with subscription URLs or Clash secrets require
  `sensitive_read`.
- Macro: a mutation without `scope` fails to compile.
- HTTP: each strategy's checks from [§4.4](#44-request-checks-per-strategy),
  DNS-rebinding Host values, cross-origin and `text/plain` posts, bearer on
  static routes rejected, loopback not exempt under LAN, plain HTTP refused
  under LAN.
- Direct HTTP routes: a proxy-only token cannot download logs, subscribe to
  connection details or receive read events. Named unauthorized events acquire
  no receiver; unfiltered subscriptions and resets omit unauthorized payloads.
  Resource-session controls cannot authenticate or broaden a principal.
- Delivery gates: before R4, production token-login/bearer admission and
  `local_open` remain unavailable; LAN cannot be enabled before the same
  authorization checks pass.
- Local socket: second OS user rejected (Unix CI), pipe DACL (Windows CI),
  owner release on disconnect.
- CLI: integration tests against a real server assembled with core test
  support, on both transports; certificate pinning and mismatch; exit codes;
  `--json` stability snapshots; curated params validated against the schema.
- CLI targets: with a live local socket and a selected remote HTTP context,
  every call uses HTTP. Connection failures on either target never contact the
  other target. Cover explicit context/environment precedence and incompatible
  transport overrides; potentially submitted mutations are never replayed.
- MCP: tool list per scope set; read-only default; schema file freshness.
- Boundary gate: the CLI links no core crate; no Tauri dependency outside the
  shell.

## 11. Decisions

| #   | Question                      | Decision                                                                                                                 |
| --- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| 1   | How a browser logs in         | Launch link and token login only; no pairing code, no password                                                           |
| 2   | TLS                           | Self-signed certificate; the loopback strategies default to plain HTTP; `lan_authenticated` requires HTTPS               |
| 3   | Port                          | The first start picks a random free port and persists it                                                                 |
| 4   | Scope granularity             | The scopes in §5.2, including a separate `sensitive_read`                                                                |
| 5   | Binary name                   | `nyanpasu-cli`                                                                                                           |
| 6   | CLI before the typed protocol | Ship v0 first                                                                                                            |
| 7   | MCP schemas                   | Generate them from the start; no handwritten interim                                                                     |
| 8   | Where secrets are stored      | In the backend; encrypted unless the storage is protected                                                                |
| 9   | Desktop or server first       | Desktop first                                                                                                            |
| 10  | HTTP MCP endpoint             | Not needed for now                                                                                                       |
| —   | Local socket switch           | Separate from HTTP, off by default                                                                                       |
| —   | MCP form                      | `nyanpasu-cli mcp` over stdio                                                                                            |
| —   | The three strategies          | `lan_authenticated`, `local_authenticated`, `local_open`                                                                 |
| —   | CLI target selection          | Select one target before its transport; no fallback to another context or instance                                       |
| —   | Resource ownership            | Separate principal attribution from per-connection or per-session resource lifetimes                                     |
| —   | Discovery permission          | Every resolved principal may inspect its own permission metadata without `read`; business-data scopes remain independent |

## 12. Order

The work is delivered as stacked layers. Each layer enforces the permissions of every
entry it enables, and a credential or session never broadens its principal.
Production scoped bearer or token login, anonymous access and the LAN listener stay
structurally unavailable (the strategy enum and the settings validation cannot name
them) until every request boundary and the command, direct-route and event
authorization are complete.

| Layer | Change                                                                                                                                                                                   | State           |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------- |
| 01    | R1: `remote_access` settings and persisted switch and port, one serial owner, degraded results, credential-free status, desktop-only launch URL, the debug HTTP wire rename, this design | **implemented** |
| 02    | Canonical `Scope`, query/mutation effect and mandatory mutation scopes generated with the catalog; initial sensitive-read classification                                                 | **implemented** |
| 03    | Privilege-separated application, Clash and runtime request contracts, matching frontend callers and retry-helper review                                                                  | **implemented** |
| 04    | Secret-free query/event/reset projections and read-only frontend bootstrap                                                                                                               | **partial**     |
| 05    | Shared authorization over commands, direct routes (including icons) and events; `rpc_describe`; generated JSON Schema                                                                    | **partial**     |
| 06    | Remote-access actor with protected persistence; HTTP token, browser and resource-session authentication                                                                                  | proposed        |
| 07    | TLS, `lan_authenticated`, `local_open` and the remote access UI                                                                                                                          | proposed        |
| 08    | Persisted local-socket switch and the reviewed CLI local allowlist                                                                                                                       | proposed        |
| 09    | `nyanpasu-cli`: target resolution, contexts, transports, curated commands                                                                                                                | proposed        |
| 10    | CLI packaging, PATH integration and completions                                                                                                                                          | proposed        |
| 11    | Agent skill and `nyanpasu-cli mcp`                                                                                                                                                       | proposed        |

Layer 04 is partial (spanning the stacked observation slices through log summaries). Delivered:
secret-free `get_app_settings`, `get_clash_settings` and `clash_api_get_settings` (reduced fields
become flags such as `pac_configured` and `latency_test_configured`, the controller address is a
validated socket address, nothing is a placeholder a form could write back); `get_proxies` and
delay tests without test URLs, icons or raw errors; typed `ClashReadError` reasons across
settings, proxies, delay tests, version, rules, rule/proxy providers, stream snapshots, and
connection-detail stream setup; safe status views and events (`get_configuration_status_summary`,
`get_core_status_summary`, `get_service_status_summary` and their `*SummaryChanged` events,
alongside `clash_ws` which carries no `Log` variant and `StorageResync` at `Scope::Read` vs
`StorageValueChanged` / `WindowMessage` at `Scope::SensitiveRead`), with ordinary status/shortcut
consumers migrated; safe profile listing and sync summaries (`get_profiles_listing`,
`get_profile_sync_status_summary`, `get_profile_sync_runs_summary`, `ProfileSyncReadError`), with
ordinary profile, layout, usage-label and subscription widgets migrated; and structured log summary
queries and status events (`get_core_log_status_summary`, `query_core_logs_summary`,
`query_logs_summary` at `Scope::Read` mutation effect matching leased session state,
`CoreLogsSummaryChanged`, `CoreLogReadError`) with `get_core_log_status`
marked `sensitive_read`, keyword/text/target summary oracle filters excluded/rejected,
`CoreLogLevel::Unknown` restricted to output projection (`InvalidRequest` on summary query filter), and app row
timestamps canonicalized as validated numeric strings.

Also delivered in the shared authorization slice (**partial** Layer 05): shared
`Principal`, canonical `ScopeSet`, `AuthContext`, OS user attribution (`UID`/`SID`)
for `LocalSocket`, non-secret bootstrap `Session` principals for HTTP (separate
from `access_token` and `nyanpasu_http_session`), audited shared authorization in
`nyanpasu-rpc::policy` (authentication resolution / missing HTTP context `401
unauthorized` before availability lookup, then `unknown_method` -> `unsupported`
-> `403 forbidden`), direct-route authorization (`/bridge/logs/archive` at
`sensitive_read`; `/bridge/connection-details` and `/bridge/icons/{*path}` at
`read`), named/unnamed `/bridge/events` SSE target/capability/scope filtering with
`resync` and revocation cancellation, `LocalServer` `Subscribe` admission before
snapshot/stream allocation, canonical `rpc_describe` (`allowed: bool` reflecting
target, capability, native linkage, and caller scopes, with the `own-only`
self-inspection exception), the `LocalSocket` allowlist expanded to 14
operations (adding `rpc_describe`), and TanStack Query `shouldRetryRpcQuery`
non-retryable failure handling in `@nyanpasu/query`.

Still open, and mandatory security gates before any new principal (scoped tokens,
anonymous access, or LAN exposure) is admitted: the following response-privacy
slice implementing global principal-aware sanitization across remaining non-read
error paths, `MutationOutcome::CommittedDegraded`, and `application_error_ref`
(plus remaining safe-read classifications); generated JSON Schema in a separate
slice after response privacy; and role-aware WebUI conditional features (such as
custom CSS from protected storage and splitting editable profile/config/log routes
whose full viewers stay on `sensitive_read`). No scoped end-to-end login exists yet.

Layer 01 corrections to the original sketch: the strategy enum has one variant so
unimplemented strategies are unrepresentable rather than rejected late; the HTTP status
no longer carries the bootstrap credential, which is available only through the
desktop-only `create_http_launch_url`; and the debug HTTP command, error domain and
messages were renamed in one change with every caller, the catalog and the bindings.
The headless server keeps its `--http` flag for now; it adopts the persisted settings
with the later layers. CLI v1 on protocol DTOs (R8) is deferred beyond the thirteen
existing typed operations.
