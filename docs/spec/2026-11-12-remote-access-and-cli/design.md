# Remote access, CLI, agent skill and MCP: design

Status: **proposed (revised)**. This design replaces the earlier 13+ layer
breakdown with a streamlined 7-PR plan targeting `main`. The maintainer's
decisions are recorded in [§11](#11-decisions) and the PR plan in
[§12](#12-order).

This design covers four connected pieces:

1. HTTP access control: a persisted switch (off by default) and three exposure
   strategies, with scoped WebUI support across permission levels.
2. Command capabilities: what each caller may call, over which transport.
3. `nyanpasu-cli`, shipped with the app, talking to a running shell over HTTP or
   a local socket (Unix domain socket, Windows named pipe).
4. An agent skill built on the CLI. (The CLI's MCP server was removed before
   release; see [§9.2](#92-mcp).)

It refines phase E of the
[platform extraction plan](../../design/application-platform-extraction.md#next-stage)
and follows [Unified RPC](../../development/rpc.md) and
[Architecture and ownership](../../development/architecture.md). The desktop
shell comes first; `nyanpasu-server` reuses the same pieces in E4.

## 1. Current state (`main @ ca620b9a0`)

Phase E of the platform extraction has delivered the neutral RPC layer this
design builds on: `nyanpasu-rpc-protocol` (command catalog, owners, frames,
typed operations), `nyanpasu-rpc` (registry, `dispatch_audited`, HTTP and
local-socket transports), `nyanpasu-single-instance::endpoint`, and the headless
`nyanpasu-server`. The CLI (E4c) and broader typed protocol (E1b) are not
delivered yet.

| Fact                                                                                                                                                                                             | Source                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| The HTTP listener is owned by `HttpServerActor` (`nyanpasu-rpc/src/http/`) with `DebugHttpControl` (`nyanpasu-core/src/client/debug_http.rs`). It is an unpersisted runtime toggle starting off. | `nyanpasu-rpc/src/http/mod.rs`; `nyanpasu-core/src/client/debug_http.rs` |
| It binds `127.0.0.1:0` only. Auth is a per-start UUID passed once via `?access_token=` on `GET /`, exchanged for an `HttpOnly; SameSite=Strict` cookie; Host and Origin must match.              | `nyanpasu-rpc/src/http/mod.rs`                                           |
| `nyanpasu_http_session` parses any client-supplied UUID directly as `RpcOwner` (or invents a random one). It is not bound to an authenticated principal.                                         | `nyanpasu-rpc/src/http/routes.rs`                                        |
| Routes: `/bridge/rpc`, `/bridge/events` (SSE), `/bridge/logs/archive`, `/bridge/connection-details` (SSE), `/bridge/icons/{*path}`. Direct routes lack scope enforcement.                        | `nyanpasu-rpc/src/http/routes.rs`                                        |
| Exposure is per-target opt-in: `rpc(http)` (100 declarations) and `rpc(local)` (13 declarations) in `command_catalog.rs`, with no per-caller scope or effect metadata yet.                       | `nyanpasu-macro/src/unified_command.rs`; `command_catalog.rs`            |
| Only `hotkeys` and `enable_service_mode` are guarded out of `patch_app_config`; system proxy, TUN, core selection, Clash secret and others still pass general patches over HTTP.                 | `nyanpasu-rpc/src/commands/runtime.rs`                                   |
| The local socket transport exists (framing, negotiation, per-connection owners, streams) and is gated by `NYANPASU_LOCAL_RPC=1` on desktop or always on in `nyanpasu-server`.                    | `nyanpasu-rpc/src/local/`; `backend/tauri/src/setup/local_rpc.rs`        |
| Every dispatched call passes `dispatch_audited` (HTTP at INFO). Auth rejections happen outside it.                                                                                               | `nyanpasu-rpc/src/registry.rs`; [rpc.md](../../development/rpc.md)       |
| Precedent for agent skills: core archives ship skills, and `CoreAgentSkill` reports each skill's folder for agents to link.                                                                      | `nyanpasu-core/src/client/agent_skills.rs`                               |
| TypeScript bindings and TanStack Query hooks are generated from the Specta collection by `specta_export::tests::export_typescript_bindings`.                                                     | `backend/tauri/src/specta_export.rs`; [rpc.md](../../development/rpc.md) |
| Sidecars ship through Tauri `externalBin`.                                                                                                                                                       | `backend/tauri/tauri.conf.json`                                          |

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
- One command-capability model covers the WebUI (preserving multi-privilege
  full, scoped, and read-only/`local_open` tiers), CLI over HTTP, CLI over the
  local socket, and MCP.
- `nyanpasu-cli` ships with every desktop bundle and drives a running desktop
  app; later the same binary drives `nyanpasu-server`.
- ~~MCP is a CLI subcommand, `nyanpasu-cli mcp`, over stdio.~~ Removed before
  release; the app will serve agents over streamable HTTP ([§9.2](#92-mcp)).
- Every authenticated call is attributable to a principal in the audit log.

Non-goals for this stage:

- A multi-user account system with passwords and roles, or pairing codes.
- Exposing the app to the internet. LAN is the widest supported exposure;
  anything beyond is the user's reverse proxy and their responsibility.
- An HTTP MCP endpoint for remote agents.
- Elevation, OS integration or shell capabilities over HTTP. These stay
  desktop-only, as today.

## 3. Settings model

A persisted section of the application config, owned by the application config
source like any other setting:

```yaml
remote_access:
  http:
    enabled: false # master switch, default off
    strategy: local_authenticated # local_authenticated | local_open | lan_authenticated
    port: null # null until the first start picks a free port and persists it
    lan_bind: null # null: all interfaces; or a specific address
    tls: http # http | self_signed; lan_authenticated requires self_signed
    serve_webui: true # API-only when false
    anonymous_permissions: {} # added to an anonymous loopback caller, at most the operator role
  local_socket:
    enabled: false
```

Progressive schema introduction:

- PR 1 (`remote-access-foundation`) introduces `remote_access.http.{enabled, strategy, port, serve_webui}` with `local_authenticated` as the only representable strategy, so an unimplemented strategy cannot be saved or loaded before its enforcement exists.
- PR 4 (`http-auth-and-scoped-ui`) adds `local_open` and its anonymous grant, now
  `anonymous_permissions` (see [§5](#5-command-capabilities)).
- PR 5 (`local-socket-cli`) adds `local_socket.enabled`.
- PR 7 (`lan-tls`) adds `lan_authenticated`, `lan_bind`, and `tls`.

Rules:

- `strategy` only matters while `enabled` is true. Turning the switch on keeps
  the previously chosen strategy; the first time it defaults to
  `local_authenticated`, matching today's behavior.
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
  result of the commit (`http_server_unavailable`), not a rollback, and the
  status reports the fault. A single serial owner (`RemoteAccessClient`, an
  actor in `nyanpasu-core`) runs commit, listener reconfiguration, and initial
  port persistence so concurrent edits cannot race or overwrite newer values.
- `remote_access` itself can only be patched through Tauri IPC and the local
  socket through a dedicated `remote:write` command, never over HTTP (see
  [§5.3](#53-hard-rules)). General config patches reject this section during
  decoding via the concrete `ApplicationPreferencesPatch` contract, preventing a
  remote caller from widening its own exposure or locking the owner out.
- The development frontend proxy (`Frontend::Dev`) stays a debug-only feature
  and is not part of these settings.

## 4. Authentication and WebUI isolation

### 4.1 Principals and resource ownership

Every request resolves to exactly one principal before dispatch:

| Principal          | How it is established                                      | Typical caller               |
| ------------------ | ---------------------------------------------------------- | ---------------------------- |
| `Desktop(window)`  | Tauri IPC, process boundary                                | Desktop webview              |
| `LocalSocket(uid)` | Accepted connection on the same-user socket                | `nyanpasu-cli`               |
| `Token(id)`        | `Authorization: Bearer <token>` over HTTP                  | CLI/scripts over HTTP        |
| `Session(id)`      | Browser session cookie, created by a launch link or login  | WebUI in a browser           |
| `Anonymous`        | Only in `local_open`, loopback request without credentials | Local scripts, local browser |

The principal identifies the authenticated caller for authorization and audit.
`RpcOwner` identifies a resource lifetime, such as a log-reading cursor. They
are derived cleanly without a separate resource-session endpoint or lease actor:

| Caller       | Derived `RpcOwner`                   | Cleanup mechanism                                              |
| ------------ | ------------------------------------ | -------------------------------------------------------------- |
| Desktop      | `desktop:{window}`                   | Window close / app cleanup                                     |
| Local socket | `local:{uid}:{conn_id}`              | Connection disconnect cleanup                                  |
| HTTP (all)   | `http:{principal_id}:{client_nonce}` | Existing 45s logging lease + per-principal stream cancellation |

For HTTP callers, `principal_id` is always server-resolved from the
authenticated principal (`session:<id>`, `token:<id>`, or `anonymous`) — a client
can never forge or cross into another principal's owner namespace. `client_nonce`
comes from an optional client-supplied `X-Nyanpasu-Client-Id` header (validated
as a short opaque identifier, defaulting to `default` when omitted). Each
`nyanpasu-cli` process generates an in-memory UUID for this
header, and each browser tab generates one in `sessionStorage`, keeping
concurrent clients using the same token independent. Abandoned log sessions
expire automatically through `nyanpasu-logging`'s existing 45-second lease, and
revoking or expiring a token or session immediately closes all active SSE
streams for that principal via per-principal cancellation.

### 4.2 Access tokens

- Long-lived, named, created by the owner from the desktop GUI or
  `nyanpasu-cli remote token create <name> --role <viewer|operator|admin>` (or
  repeated `--permission <resource:access>` for a custom map) over the local
  socket.
- Format `npk_<id>_<secret>`: a public id for listing, revoking and audit, and a
  256-bit random secret. Only a SHA-256 hash of the secret is kept (high
  entropy makes a slow hash unnecessary); the secret is shown once.
- Each token carries a grant ([§5](#5-command-capabilities)) and an optional
  expiry. Revocation or expiry invalidates its derived browser sessions and ends
  active SSE streams for that principal. It prevents new admissions without
  cancelling already-admitted business work.
- Storage follows [§4.6](#46-secrets-at-rest).

### 4.3 Browser login and scoped WebUI isolation

A browser cannot send a bearer header on navigation, so the WebUI uses a
session cookie (`HttpOnly; SameSite=Strict; Path=/`, plus `Secure` under TLS).
There are two ways to obtain one:

1. **Launch link** (all strategies): the desktop "Open WebUI" action mints a
   one-time code, valid for 60 seconds, and opens `/?code=…`. This replaces
   today's per-start `access_token`, which is reusable for the whole run.
2. **Login with a token** (all authenticated strategies, the only way from
   another device on the LAN): a minimal `/login` page accepts an access token
   and exchanges it for a session cookie bound to that token's grant. A launch
   code opens an `admin` session.

The WebUI preserves full multi-privilege behavior (**full-access**, **scoped
token**, and **read-only / `local_open`** tiers) without complex in-memory
principal hot-swapping:

- **Page-lifetime principal binding.** Login (`/?code=…` or `/login` form) and
  logout/re-authentication always perform a full document navigation
  (`window.location.replace`), so each browser page runtime binds to one
  immutable principal for its entire lifetime.
- **Pre-mount bootstrap gate.** In `frontend/nyanpasu/src/pages/__root.tsx`, a
  lightweight `<AuthBootstrapGate>` runs **before** `<NyanpasuQueryProvider>`,
  `<CustomCssProvider>`, and app routes mount:
  - It calls `rpc_describe()` once.
  - On `401` (unauthenticated under an authenticated strategy), it renders or
    redirects to `/login` without mounting app providers.
  - On success, it compares the resolved non-secret principal+permission key
    (`principal.kind + principal.id + permissions`) against a stored browser
    marker in `localStorage`. If the principal or its permissions changed (for
    example, switching tokens or dropping into `local_open`), it synchronously
    clears persisted `nyanpasu-kv-*` / theme `localStorage` keys and calls
    `queryClient.clear()` **before** rendering `<NyanpasuQueryProvider>` or
    `<CustomCssProvider>`. Because providers are not yet mounted, there are zero
    in-flight queries, zero pending KV writes, zero injected `<style>` elements,
    and zero stale module closures.
- **Runtime revocation handling.** When any RPC call returns `401` or an SSE
  stream closes due to revocation/expiry, the client calls `rpc.dispose()`,
  clears `nyanpasu-kv-*` and the principal marker, and navigates via
  `window.location.replace('/login')`.
- **Permission-aware UI gating.** A `useCapabilities()` hook exposes the
  immutable `rpc_describe()` snapshot so routes, controls, and storage-backed UI
  features gate cleanly on permissions written `resource:access`:
  - Pages and viewers gate on the `read` of the documents they show (for
    example `profile:read` for profile contents, `settings:read` for custom CSS
    storage, `logs:read` for log viewers).
  - Mutation controls gate on the resource's `write` (via both command
    `allowed` flags and permission checks).
  - `remote` controls are rendered only on Desktop, matching HTTP exposure
    rules.

### 4.4 Request checks per strategy

| Check                                | `local_open`                | `local_authenticated` | `lan_authenticated`                                        |
| ------------------------------------ | --------------------------- | --------------------- | ---------------------------------------------------------- |
| Bind                                 | `127.0.0.1`, `::1`          | `127.0.0.1`, `::1`    | `lan_bind` or all interfaces                               |
| Scheme                               | HTTP, or HTTPS if opted in  | Same                  | HTTPS only                                                 |
| Host header                          | Must be the bound authority | Same                  | Any address the listener is bound to, or a configured name |
| Origin (when present)                | Must equal the Host origin  | Same                  | Same                                                       |
| Credential                           | Optional                    | Required              | Required, also from loopback                               |
| JSON `content-type` on `/bridge/rpc` | Required                    | Required              | Required                                                   |

Notes:

- `lan_authenticated` never trusts the peer address. A reverse proxy makes
  every request look like loopback, so loopback is not an exemption.
- Requiring `application/json` forces a CORS preflight for cross-origin
  browser requests; the server answers no preflight, so a malicious page cannot
  post to `local_open`. Origin checks cover the rest.
- `local_open` trusts **every local process of every OS user** on the
  machine, because loopback TCP is not user-scoped. The settings UI states
  this, and an anonymous caller holds only capability discovery unless
  `anonymous_permissions` adds more. Users who want an
  unauthenticated CLI should use the local socket, which is same-user only.
- Credential verification uses constant-time comparison over SHA-256 hashes;
  `401` responses carry no detail. Because secrets have 256 bits of random
  entropy, no per-peer lockout/rate-limiter state machine is needed.
- Authentication failures are logged at WARN on a separate target
  (`nyanpasu::remote_access`) with the peer address and the public token id if
  one was parsed, never the secret. Successful calls go through
  `dispatch_audited` as today, with the principal as the caller.

### 4.5 TLS (PR 7)

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

The remote-access owner keeps token SHA-256 hashes, session metadata, and (in PR 7) the TLS private key in a single plain owner-only backend file:
`<data_dir>/remote-access/auth-secrets.json` (directory `0700` and file `0600` on
Unix; standard per-user application data directory on Windows, matching how Clash
secrets and subscription URLs are stored).

These records are excluded from config backups and exports, and config roots
pointing inside `remote-access` are rejected. No OS keyring AEAD wrapper or
filesystem ancestor-ACL walker is required; a restored installation creates new
tokens and a new certificate.

## 5. Command capabilities

The permission model below replaced the original scope set before the stack was
released; its design and the reviewed change list are in the
[permission namespace spec](../2026-11-16-rpc-permission-namespace/design.md).

### 5.1 Two independent axes

- **Exposure (per target):** whether a transport offers the command at all. This
  is per-target opt-in: `rpc(http)` (WebUI/HTTP) and `rpc(local)` (local
  socket). Tauri IPC exposes everything. A command not exposed on a target
  returns a structured `unsupported` error from the registry.
- **Permission (per caller):** what a caller may do among the exposed commands.
  Each command requires exactly one permission, a resource and an access level
  written `proxy:write`; each caller holds at most one level per resource.

A call succeeds only if the command is exposed on the transport **and** the
caller holds its permission. The CLI over HTTP therefore sees the HTTP
allowlist and the CLI over the socket sees the local allowlist. "CLI" is not a
transport and gets no allowlist of its own.

### 5.2 Permissions and roles

| Resource   | Covers                                                                                |
| ---------- | ------------------------------------------------------------------------------------- |
| `meta`     | Capability discovery (`rpc_describe`) only; every resolved caller holds `meta:read`   |
| `proxy`    | Proxy groups and nodes, selection, latency tests, the proxy mode, closing connections |
| `profile`  | Profiles and subscriptions, chains, activation, the proxy group layout                |
| `settings` | Ordinary application and Clash preferences; frontend key-value storage                |
| `system`   | Core lifecycle, system proxy, TUN, transparent proxy, service mode, OS integration    |
| `logs`     | Application and core logs                                                             |
| `traffic`  | Traffic, connection lists and their history                                           |
| `remote`   | `remote_access` settings, tokens, sessions, certificates; never over HTTP             |

`write` includes `read`. A query requires its resource's `read` and a mutation
its `write`, with reviewed exceptions (latency tests require `proxy:write`; log
sessions and connection-detail subscriptions require `read`). Declaration,
until the namespace tree derives it:

```rust
#[nyanpasu_rpc::rpc(http, local, permission = proxy:write)]
pub async fn select_proxy(/* … */) -> Result<()> { /* … */ }

#[nyanpasu_rpc::rpc(http, local, query, permission = profile:read, sensitive)]
pub async fn read_profile_file(/* … */) -> Result<String> { /* … */ }
```

Roles are named permission sets; admission never sees them:

| Role        | Holds                                                                   | Held by                                             |
| ----------- | ----------------------------------------------------------------------- | --------------------------------------------------- |
| `anonymous` | Nothing beyond `meta:read`, plus the configured `anonymous_permissions` | An HTTP request without a credential (`local_open`) |
| `viewer`    | `read` on `proxy`, `profile`, `settings`, `system`, `logs`, `traffic`   | Tokens                                              |
| `operator`  | `viewer`, plus `write` on `proxy`, `profile`, `settings`, `traffic`     | Tokens                                              |
| `admin`     | `operator`, plus `write` on `system` and `logs`                         | Tokens; launch-code browser sessions                |
| `owner`     | `admin`, plus `remote:write`                                            | The desktop window and the same-user local socket   |

- A token stores a grant, `{ "role": "viewer" }` or a custom map
  `{ "permissions": { "proxy": "write", "logs": "read" } }`. A role follows later
  changes to its definition; a custom map stays as granted. Only `viewer`,
  `operator` and `admin` can be granted, and a custom map never holds `remote`.
  The grant is validated when the token is issued and when the stored record is
  loaded. A browser session opened with a token holds the token's grant.
- `sensitive` marks a command whose response may carry credentials,
  subscription URLs, configuration text or diagnostics. It never takes part in
  admission: a sensitive read requires its domain's `read`. The token dialog,
  the anonymous settings and `nyanpasu-cli remote token create` list the
  sensitive commands a grant reaches.
- `nyanpasu_config::application::{Resource, Access, Permission, Permissions, Role}`
  are the single definitions used across config validation
  (`anonymous_permissions`), core auth, and `nyanpasu-rpc-protocol`.

One permission per command requires matching request and response boundaries:

- **Privilege-separated patches.** General `patch_app_config` and
  `patch_clash_config` accept ordinary preferences only
  (`ApplicationPreferencesPatch` / `ClashPreferencesPatch`). Privileged fields
  (`enable_system_proxy`, TUN, transparent proxy, service mode, core selection)
  and `remote_access` move to dedicated commands and patch contracts; mixed,
  `false`, empty-collection, and same-value unauthorized writes are rejected
  atomically before handler admission across all transports.
- **Secret-free companion reads.** Queries exposing subscription URLs, Clash
  secrets, raw profile/config text, or unsanitized logs are marked `sensitive`;
  secret-free companion queries (safe settings, safe status/profile/log
  summaries) carry the useful part without the marking.
- **Response and outcome privacy.** Until the redaction layer has its own
  rules, a caller sees an unredacted response only when it holds `write` on the
  resource it called; others receive bounded `PublicFailure` / redacted
  `CommittedDegraded` views without filesystem paths, URLs, credentials, or raw
  internal diagnostics.

### 5.3 Hard rules

- `remote` is never exposed over HTTP. Remote access is managed from the
  desktop GUI or the local socket only.
- Elevation, OS integration and shell capabilities keep their existing
  restrictions; a permission never grants a capability the shell does not
  provide.
- `local_open` anonymous callers never hold more than the `operator` role,
  whatever `anonymous_permissions` says: validation rejects such a patch, and
  the listener caps the stored value.
- `LocalSocket(uid)` is the owner: it is the same OS user who owns the app.

### 5.4 Capability discovery

`rpc_describe`, exposed on every target, returns:

```json
{
  "protocol": 2,
  "server": { "kind": "desktop", "version": "2.0.0" },
  "transport": "http",
  "principal": { "kind": "token", "id": "a1b2", "name": "laptop-cli" },
  "role": null,
  "permissions": { "proxy": "write", "logs": "read" },
  "commands": [
    {
      "name": "select_proxy",
      "effect": "mutation",
      "permission": { "resource": "proxy", "access": "write" },
      "sensitive": false,
      "allowed": true
    }
  ]
}
```

`rpc_describe` requires `meta:read`, which every resolved caller holds, so any
resolved caller may inspect its own metadata; it grants no business-data access,
leaks no other principal's state, and admits no invalid credential. The CLI uses
it to check protocol compatibility and filter subcommands; payload schemas for
curated operations live in the CLI's static table ([§8.4](#84-v0-before-the-typed-protocol)).

### 5.5 Direct HTTP routes and streams

Routes outside `/bridge/rpc` pass through the same audited authorization policy
before reading files, generating archives, or acquiring stream receivers:

| Entry                        | Required permission and admission                                                                        |
| ---------------------------- | -------------------------------------------------------------------------------------------------------- |
| `/bridge/logs/archive`       | `logs:read` (sensitive) before generating or reading diagnostic archives                                 |
| `/bridge/connection-details` | `traffic:read` before subscribing                                                                        |
| `/bridge/icons/{*path}`      | `system:read` before serving profile/group icons                                                         |
| `/bridge/events?name=…`      | Explicit per-event exposure and permission (`storage-value-changed-event` at `settings:read`, sensitive) |
| `/bridge/events` (unnamed)   | Deliver only HTTP-exposed events whose permission the caller holds                                       |

Active HTTP SSE streams register under their principal's cancellation token and
stop immediately when the backing token or session is revoked or expires.

## 6. Transports

### 6.1 HTTP

- API-only mode: the router starts without frontend assets when
  `serve_webui: false` or when no assets exist (`nyanpasu-server`).
- The bearer header is accepted on `/bridge/*` only; static assets accept only
  sessions (or anonymous under `local_open`).
- `HttpServerActor` receives the whole `remote_access.http` value and restarts
  the listener when bind-relevant fields change, keeping graceful-drain
  behavior.
- The authenticator is a port consumed by the HTTP transport and implemented by
  the remote-access owner in core, avoiding any dependency cycle.

### 6.2 Local socket

| Platform | Endpoint                                                                        | Protection                                               |
| -------- | ------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Linux    | `$XDG_RUNTIME_DIR/clash-nyanpasu/rpc.sock`, falling back to the app runtime dir | Directory `0700`, socket `0600`, `SO_PEERCRED` uid check |
| macOS    | App data dir `run/rpc.sock`                                                     | Same, `getpeereid` uid check                             |
| Windows  | `\\.\pipe\clash-nyanpasu-rpc-<user SID hash>`                                   | DACL for the current user only, client token check       |

- Reuses `nyanpasu-rpc/src/local/` (`LocalServer` over
  `nyanpasu-single-instance::endpoint`), changing desktop activation from
  `NYANPASU_LOCAL_RPC=1` to persisted `remote_access.local_socket.enabled`
  reconfiguration with clean drain on disable.
- Wire: length-prefixed JSON frames with the same envelope as `/bridge/rpc`
  (`{ "id", "method", "params" }` -> `{ "id", "result" }` or
  `{ "id", "error": RpcError }`) plus stream subscriptions. One connection is
  one `RpcOwner`; closing it releases its subscriptions and owned resources.
- Enters `dispatch_audited` at INFO.
- The endpoint path is derived from `nyanpasu-paths`, so the CLI finds it
  without a discovery file.

## 7. Ownership and packages

| Piece                                                                           | Kind                 | Home                                                                                                                 |
| ------------------------------------------------------------------------------- | -------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `remote_access` settings schema, validation, the permission model and roles     | Pure                 | `nyanpasu-config`                                                                                                    |
| `RemoteAccessActor` / `RemoteAccessClient`: tokens, sessions, launch codes, TLS | Actor                | Core package family; exposed through `NyanpasuClient` domain operations                                              |
| Secret file storage (`auth-secrets.json`) and backup exclusion                  | Port + adapter       | Port in core, plain owner-only adapter in `nyanpasu-os` / `nyanpasu-paths`                                           |
| Permission, sensitive marking and exposure metadata (generated resource table)  | Protocol             | `nyanpasu-rpc-protocol`                                                                                              |
| Admission (exposure and capabilities, then permissions) and response redaction  | Registry + transport | `Authority` and the redaction layer in `nyanpasu-rpc`, applied inside `dispatch_audited`, direct routes, and streams |
| HTTP listener, bearer/session/nonce extraction, Host/Origin, TLS                | Transport            | `nyanpasu-rpc/src/http/`                                                                                             |
| Local-socket listener and client                                                | Transport            | `nyanpasu-rpc/src/local/`                                                                                            |
| `nyanpasu-cli` and its curated command table                                    | Client               | `backend/nyanpasu-cli`, production links protocol/transport only (`nyanpasu-core` in dev-deps only)                  |

No Tauri type enters any of these except the desktop composition root. The
remote-access owner holds no reference to the HTTP server; the server consumes
the authenticator port.

## 8. `nyanpasu-cli`

### 8.1 Connection

- Contexts, kubectl-style, in `cli.toml` under the user config dir: name, target
  kind (`local_socket` or `http`), endpoint or HTTP URL, token reference, and
  pinned TLS fingerprint. `nyanpasu-cli context add|use|list|remove`;
  `--context <name>` selects one invocation without changing the default.
- Target resolution: `--context`, else `NYANPASU_URL`, else current context,
  else default local socket.
- `--transport auto|socket|http` selects a transport for the resolved target
  only. An incompatible transport is a usage error, and a failed connection
  never falls back to another context or instance.
- CLI tokens are stored in the OS keychain when available, otherwise in a `0600`
  file. `NYANPASU_TOKEN` overrides the selected HTTP target's credential. Tokens
  are never accepted as plain positional/flag arguments to keep them out of shell
  history.
- On connect, the CLI sends an in-memory per-process `X-Nyanpasu-Client-Id`
  header on HTTP, calls `rpc_describe`, and refuses incompatible protocol
  versions.

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
nyanpasu-cli skill     path | install [--agent <name>]
nyanpasu-cli completion <shell>
```

### 8.3 Output and exit codes

- Human-readable tables by default; `--json` prints the RPC result unchanged.
  Streams print one JSON object per line.
- Degraded mutations (`CommittedDegraded`) print their degradation and exit with
  code `6`, never `0`.

| Exit | Meaning                                                   |
| ---- | --------------------------------------------------------- |
| 0    | Success                                                   |
| 1    | Application error (`RpcError` from the domain)            |
| 2    | Usage error                                               |
| 3    | Not reachable: app not running, socket disabled, HTTP off |
| 4    | Not authenticated or permission denied                    |
| 5    | Unsupported on this target or shell                       |
| 6    | Applied with degradation                                  |

### 8.4 v0 before the typed protocol

The extraction plan schedules full protocol DTOs in E1b, after which CLI v1 can
bind directly to typed request/response structs. To ship sooner without adding a
1,600+ line custom AST schema generator:

- **CLI v0** keeps a small curated static command table (~15–20 operations):
  RPC method name, permission, description, and compact JSON input schema. `call <method> [<json>]` covers every other command.
- **Integration test verification:** `nyanpasu-cli` includes `nyanpasu-core` as
  a `dev-dependency` solely to spin up a real in-process RPC server and exercise
  every curated command end-to-end, catching any renamed field or
  stale schema in CI before users ever hit it.

### 8.5 Shipping

| Platform | Install location                                        | PATH                                                                                                           |
| -------- | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Windows  | Next to the app through `externalBin`                   | NSIS/MSI option "Add the CLI to PATH"; Scoop `bin` shim                                                        |
| macOS    | `Contents/MacOS/nyanpasu-cli`                           | GUI action "Install command-line tool" links it into `~/.local/bin` or prints the command for `/usr/local/bin` |
| Linux    | `/usr/bin/nyanpasu-cli` in deb/rpm; inside the AppImage | Packages: already on PATH; AppImage: GUI action as on macOS                                                    |

The CLI version equals the app version. In production builds, the binary links
only protocol and transport crates (no core, no script engines), enforced by the
boundary gate.

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
- Tests verify that every command named in the skill exists in `nyanpasu-cli`.

### 9.2 MCP

> **Removed.** The stdio server below was removed before release: a server inside
> the CLI binary pinned agents to a CLI that the app's own updates replace, and the
> server cannot tell an MCP connection from any other caller. Agents will be
> served over streamable HTTP by the app instead (TODO; see the permission
> namespace spec, extensions E-2 and E-4). The original design is kept below for
> reference.

- `nyanpasu-cli mcp` is a stdio MCP server reusing the CLI's connection logic,
  contexts, and credentials.
- **Read-only by default:** it registers tools only for `read` entries in the
  curated static tool table. `--scope proxy --scope profile …` adds scopes,
  always intersected with the principal's scopes from `rpc_describe`.
  `remote_admin` is never offered.
- **Curated static tool table (~15–20 tools):** shared with CLI v0, carrying
  handwritten descriptions, compact JSON input schemas, and MCP annotations
  (`readOnlyHint` for queries, `destructiveHint` for mutations that drop
  connections or stop the core). An opt-in `--raw` flag exposes a generic
  `call_method` tool for uncurated commands (still bounded by the principal's
  scopes and never offering `remote_admin`).
- Because the table is compiled into `nyanpasu-cli`, `nyanpasu-cli mcp` needs no
  embedded 20K-line generated schema file and no `nyanpasu-core` runtime
  dependency. Accuracy is enforced by the real core-backed integration tests
  described in [§8.4](#84-v0-before-the-typed-protocol).
- Each MCP process establishes its own `RpcOwner` (dedicated local socket
  connection or HTTP `X-Nyanpasu-Client-Id` nonce) so concurrent MCP instances
  do not collide on log cursors or stream lifetimes.

## 10. Verification

- **Owner & tokens:** token creation, SHA-256 verification, browser sessions,
  launch-code single use/expiry, and revocation (including ending active HTTP SSE
  streams for that principal without cancelling admitted work) with a fake
  clock.
- **Derived ownership:** concurrent local-socket clients and concurrent HTTP
  clients sharing a token receive distinct `RpcOwner`s; `X-Nyanpasu-Client-Id`
  cannot cross into another principal's owner namespace; logging's 45s lease
  reaps abandoned cursors.
- **Secrets at rest:** `<data_dir>/remote-access/auth-secrets.json` enforces
  `0700`/`0600` on Unix, is excluded from backups even concurrently with first
  write, and rejects overlapping config dirs.
- **Settings:** validation (`lan_authenticated` requires `self_signed`,
  anonymous permissions capped at the operator role), first-start port persistence, degraded status when a
  persisted port is busy, and serial actor ordering.
- **Authorization & privacy:** exposure × permission matrix across all callers
  (`remote` rejected over HTTP, `unsupported` for unexposed commands);
  atomic rejection of unauthorized or mixed general patches; the pinned
  sensitive set, which takes no part in admission; `PublicFailure` and
  `CommittedDegraded` redaction for callers without `write` on the called
  resource; generator rejection of declarations lacking `permission`.
- **HTTP & direct routes:** Host/Origin/JSON checks across strategies; bearer on
  static routes rejected; loopback not exempt under `lan_authenticated`; direct
  route and SSE event filtering (`resync` and revocation stop).
- **Scoped WebUI isolation:** pre-mount `<AuthBootstrapGate>` redirects `401` to
  `/login` without mounting providers; switching principal/scope synchronously
  clears `nyanpasu-kv-*`, theme storage, and `QueryClient` before mount;
  runtime `401` disposes RPC and navigates to `/login`; `useCapabilities()`
  hides pages whose documents the caller may not read, unauthorized
  mutations, and HTTP `remote` UI.
- **Local socket & CLI:** same-user socket rejection (Unix `SO_PEERCRED` /
  Windows pipe DACL); CLI target precedence with zero cross-target fallback;
  TLS pinning and mismatch error (PR 7); exit codes 0–6; every curated CLI
  command exercised against a real core-backed test server; skill command check;
  production CLI links no core crate.

## 11. Decisions

| #   | Question                      | Decision                                                                                                                                                                   |
| --- | ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | How a browser logs in         | 60s single-use launch link and token login only; no pairing code, no password                                                                                              |
| 2   | WebUI privilege tiers         | Preserve multi-privilege UI (full, scoped token, `local_open`); isolate via full-navigation + pre-mount gate                                                               |
| 3   | Resource ownership            | Derived `http:{principal_id}:{client_nonce}` + existing 45s logging lease; no resource-session endpoints/cookies                                                           |
| 4   | Secrets at rest & auth limits | Plain owner-only `auth-secrets.json` (excluded from backups); no keyring AEAD, ACL walkers, or peer rate limiter                                                           |
| 5   | CLI & MCP schemas             | Curated static command/tool table (~15–20 ops) + raw `call`/`call_method`, verified by core integration tests                                                              |
| 6   | TLS & LAN                     | Self-signed cert, pinned in CLI; deferred with `lan_authenticated` to the final PR after local/CLI/MCP ship                                                                |
| 7   | Port                          | First start picks a random free port and persists it                                                                                                                       |
| 8   | Scope granularity             | Canonical `Scope` enum (`read`, `sensitive_read`, `proxy`, `profile`, `settings`, `system`, `remote_admin`); superseded before release by the resource permissions of §5.2 |
| 9   | Binary name & v0              | `nyanpasu-cli`; ship v0 on JSON + static table before E1b typed DTOs                                                                                                       |
| 10  | Desktop/server & MCP form     | Desktop first (`nyanpasu-server` reuses pieces in E4); `nyanpasu-cli mcp` over stdio, superseded: removed before release in favor of app-hosted streamable HTTP (TODO)     |
| —   | Local socket switch           | Separate persisted switch (`local_socket.enabled`), off by default                                                                                                         |
| —   | CLI target selection          | Select one target before transport; zero fallback to another context or instance                                                                                           |
| —   | Discovery permission          | `rpc_describe` requires `meta:read`, which every resolved caller holds, and returns only the caller's own metadata                                                         |

## 12. Order

Delivered as a clean **7-PR stack from `main`**. Production scoped tokens,
`local_open`, and `lan_authenticated` stay structurally unavailable until PRs
1–3 establish capability metadata, patch boundaries, safe read views, and unified
authorization:

| PR  | Branch / Name              | Scope                                                                                                                                                                     | Depends on |
| --- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| 1   | `remote-access-foundation` | `remote_access` settings/switch/port, `RemoteAccessClient`, wire rename, canonical `Scope` + mandatory mutation scopes, `ApplicationPreferencesPatch` / system boundaries | —          |
| 2   | `safe-read-views`          | Secret-free settings, proxy, status, profile, sync, and log summary queries/events; `sensitive_read` classification and frontend consumer migration                       | 1          |
| 3   | `authorization-privacy`    | Unified audited `policy` over commands, direct HTTP routes (including icons), and SSE events; own-only `rpc_describe`; `ResponseVisibility`, `PublicFailure`, outcomes    | 2          |
| 4   | `http-auth-and-scoped-ui`  | `RemoteAccessAuthActor` + owner-only `auth-secrets.json`, tokens/launch-code/sessions/`local_open`, HTTP checks, `<AuthBootstrapGate>`, `useCapabilities()` UI, token UI  | 3          |
| 5   | `local-socket-cli`         | Persisted `local_socket.enabled` + reviewed local allowlist + `nyanpasu-cli` v0 (contexts, targets, curated commands, `call`, `--json`, exit codes, real-server tests)    | 4          |
| 6   | `cli-packaging-skill-mcp`  | `externalBin` packaging, NSIS/Scoop/macOS/Linux PATH + shell completions, `skills/nyanpasu-cli/`, `nyanpasu-cli mcp` over curated tool table + `--raw`                    | 5          |
| 7   | `lan-tls`                  | Self-signed cert generation, `lan_authenticated` + `lan_bind` + `tls`, CLI certificate pinning/verification, LAN settings & fingerprint UI                                | 5 (or 6)   |
