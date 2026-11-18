# Command reference

Every command accepts the global flags `--json`, `--context <name>`,
`--transport <auto|socket|http>` and `--config <file>`.

| Command                                                                       | Scope            | Effect                                     |
| ----------------------------------------------------------------------------- | ---------------- | ------------------------------------------ |
| `nyanpasu-cli status`                                                         | `read`           | Core status                                |
| `nyanpasu-cli core version`                                                   | `read`           | Version the running core reports           |
| `nyanpasu-cli core restart`                                                   | `system`         | Restarts the core and drops connections    |
| `nyanpasu-cli core switch <core>`                                             | `system`         | Switches the core and restarts it          |
| `nyanpasu-cli mode get`                                                       | `read`           | Core configuration, including the mode     |
| `nyanpasu-cli mode set <rule\|global\|direct>`                                | `proxy`          | Changes the proxy mode                     |
| `nyanpasu-cli proxy groups`                                                   | `read`           | Proxy groups and selections                |
| `nyanpasu-cli proxy select <group> <proxy>`                                   | `proxy`          | Selects a proxy in a group                 |
| `nyanpasu-cli proxy test <name>`                                              | `proxy`          | Latency test of one proxy                  |
| `nyanpasu-cli proxy test <group> --group`                                     | `proxy`          | Latency test of every proxy in a group     |
| `nyanpasu-cli profile list`                                                   | `read`           | Profiles and the active one                |
| `nyanpasu-cli profile use <id>`                                               | `profile`        | Activates a profile                        |
| `nyanpasu-cli profile update <id>`                                            | `profile`        | Refreshes a remote profile                 |
| `nyanpasu-cli profile import <url> --name <name>`                             | `profile`        | Imports a subscription                     |
| `nyanpasu-cli profile show <id>`                                              | `sensitive_read` | Profile file, which may hold credentials   |
| `nyanpasu-cli sysproxy status`                                                | `read`           | Operating system proxy                     |
| `nyanpasu-cli sysproxy on`                                                    | `system`         | Turns the system proxy on                  |
| `nyanpasu-cli tun status`                                                     | `read`           | Clash settings, including TUN              |
| `nyanpasu-cli tun off`                                                        | `system`         | Turns TUN mode off                         |
| `nyanpasu-cli conn list`                                                      | `read`           | Connections                                |
| `nyanpasu-cli conn close <id>`                                                | `proxy`          | Closes one connection                      |
| `nyanpasu-cli logs tail --level <level> --limit 50`                           | `sensitive_read` | Latest core log rows                       |
| `nyanpasu-cli traffic`                                                        | `read`           | Traffic summary                            |
| `nyanpasu-cli config get <path>`                                              | `read`           | Application settings, or one dotted path   |
| `nyanpasu-cli config patch '<json object>'`                                   | `settings`       | Patches ordinary settings                  |
| `nyanpasu-cli methods`                                                        | none             | What this caller may call (`rpc_describe`) |
| `nyanpasu-cli call <method> '<json object>'`                                  | per method       | Raw call of any allowed command            |
| `nyanpasu-cli mcp`                                                            | `read`           | Curated read-only MCP server over stdio    |
| `nyanpasu-cli mcp --scope <proxy\|profile\|settings\|system\|sensitive_read>` | `read` + scope   | Curated MCP server with an extra scope     |
| `nyanpasu-cli mcp --raw`                                                      | `read`           | Also exposes `call_method` within scopes   |
| `nyanpasu-cli completion bash`                                                | none             | Shell completion script                    |
| `nyanpasu-cli skill path`                                                     | none             | Folder that holds this skill               |
| `nyanpasu-cli skill install --agent <claude\|codex\|agents>`                  | none             | Links the skill into an agent's folder     |

A scope the caller lacks ends in exit code 4 before anything is sent. A method the
target does not expose ends in exit code 5.

The `remote` commands, such as `nyanpasu-cli remote status`, administer remote access
(the HTTP server, the local socket, tokens and sessions) and work only over the local
socket. They are not part of this skill: run them only when the user asks for one by
name.
