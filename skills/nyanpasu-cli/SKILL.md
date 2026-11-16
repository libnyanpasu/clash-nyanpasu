---
name: nyanpasu-cli
description: Inspect and control a running Clash Nyanpasu app from the terminal with nyanpasu-cli. Use it to check the core status, list or switch proxies, change the proxy mode, manage profiles and subscriptions, toggle the system proxy or TUN, read connections, logs and traffic, or call any allowed RPC method.
---

# Controlling Clash Nyanpasu with `nyanpasu-cli`

`nyanpasu-cli` talks to one running Clash Nyanpasu app (or `nyanpasu-server`)
through its local socket or an HTTP URL. It never starts the app, never retries a
change, and never falls back to another target.

## Ground rules

- Always pass `--json`. Output is the RPC result unchanged, and streams print one
  JSON object per line. Never parse the default tables.
- Start every session with `nyanpasu-cli --json status`, then
  `nyanpasu-cli --json methods`. `methods` lists what this caller may do; do not
  attempt commands it does not list.
- Treat queries as safe. Before any mutation, say what it will change and ask the
  user to confirm. Some mutations drop connections or restart the core; confirm those
  explicitly.
- `profile show` can print subscription URLs and credentials. Do not paste its output
  into replies or logs.
- Never create tokens, add contexts with tokens, or change remote-access settings (the
  `remote` commands, such as `nyanpasu-cli remote status`) unless the user asks for
  exactly that. Never put a token on a command line.
- Never run several mutations in parallel. Run one, read its result, then continue.

## Choosing the target

Resolution is strict and picks exactly one target: `--context <name>`, else the
`NYANPASU_URL` variable, else the current context, else the default local socket.
`--transport socket` or `--transport http` only checks that the resolved target uses
that transport. A failure to connect is final; do not retry against another target.
See [troubleshooting](references/troubleshooting.md) for contexts and tokens.

## Everyday commands

Read state:

- `nyanpasu-cli --json status`
- `nyanpasu-cli --json proxy groups`
- `nyanpasu-cli --json profile list`
- `nyanpasu-cli --json mode get`
- `nyanpasu-cli --json sysproxy status`
- `nyanpasu-cli --json tun status`
- `nyanpasu-cli --json conn list`
- `nyanpasu-cli --json logs tail --limit 50`
- `nyanpasu-cli --json traffic`
- `nyanpasu-cli --json config get`

Change state, after confirming with the user:

- `nyanpasu-cli --json proxy select <group> <proxy>`
- `nyanpasu-cli --json mode set <rule|global|direct>`
- `nyanpasu-cli --json profile use <id>`
- `nyanpasu-cli --json profile update <id>`
- `nyanpasu-cli --json sysproxy on` and `nyanpasu-cli --json sysproxy off`
- `nyanpasu-cli --json tun on` and `nyanpasu-cli --json tun off`
- `nyanpasu-cli --json conn close --all` drops every connection.
- `nyanpasu-cli --json core restart` drops every connection while the core restarts.

Streams (`logs tail -f`, `traffic -f`, `conn list -f`) run until interrupted. Prefer
the one-shot forms, or stop the stream after the lines you need.

Any other allowed command: `nyanpasu-cli --json call <method> '<json object>'`.
Read the method name from `nyanpasu-cli --json methods` and ask the user before
calling anything that changes state. See the [command reference](references/commands.md).

## Exit codes

| Code | Meaning                                             | What to do                                       |
| ---- | --------------------------------------------------- | ------------------------------------------------ |
| 0    | Success                                             | Continue.                                        |
| 1    | Application error from the app                      | Read the message; do not blindly retry.          |
| 2    | Usage error                                         | Fix the arguments.                               |
| 3    | Not reachable: app stopped, socket or HTTP disabled | Tell the user; do not try another target.        |
| 4    | Not authenticated, or the scope is denied           | Tell the user which scope is missing.            |
| 5    | Unsupported on this target or app version           | Pick another approach or report it.              |
| 6    | Applied, but part of the effect degraded            | The change is committed; report the degradation. |

Exit code 6 means the change took effect; do not repeat it. Read the degradation
on standard error and tell the user.

## Run it as an MCP server

`nyanpasu-cli mcp` serves the same curated operations over standard input and output
for agents that speak MCP. It is read-only by default; `nyanpasu-cli mcp --scope proxy`
adds the `proxy` tools (repeat `--scope` for additional scopes), always limited to the
caller's own scopes. Pass `nyanpasu-cli mcp --raw` to also expose a generic
`call_method` tool for uncurated commands, still bounded by the same scopes. It never
offers `remote_admin`: remote access is administered from the app, never through MCP.
