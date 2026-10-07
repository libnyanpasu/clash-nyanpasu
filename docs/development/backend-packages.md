# Backend package boundaries

Tauri is the desktop host and GUI/transport adapter. Shared application behavior belongs in `nyanpasu-core`, independent of the host.
Concrete infrastructure stays behind narrow ports and explicit construction.
See the [extraction plan](../design/application-platform-extraction.md) for the
remaining migration graph; this guide defines the current dependency rules.

| Package           | Owns                                                                                                            | Must not depend on                        |
| ----------------- | --------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `nyanpasu-config` | Domain models, profile validation, pure runtime executor and its ports                                          | Core, Tauri or egui/eframe                |
| `nyanpasu-core`   | Shared use cases, typed clients, workflows, ports, persistence primitives and capability-local non-GUI adapters | Tauri or egui/eframe                      |
| `backend/tauri`   | Desktop composition root, concrete Tauri adapters, GUI lifecycle and IPC                                        | No reverse dependency from neutral crates |

`nyanpasu-application` and `nyanpasu-platform` have been removed. Their existing
runtime/builtin implementation, original tests and filesystem/script adapters
have one owner in `nyanpasu_core::runtime::config`. Do not recreate the packages or
retain old-path wrappers to avoid updating consumers.

Shared application code receives dependencies through constructors, function
arguments or actor startup parameters. Ports belong to the consuming core
capability. Concrete non-GUI adapters may be capability-local in core when hosts
need to construct that capability directly. This does not make pure services
perform hidden IO or permit process-global caches. `runtime::config` assembles explicit snapshots
through config executor ports; its FS content source and Boa/Lua/script adapters
receive explicit directories. The synchronous build runs in a blocking context;
`RuntimeConfigScriptRunner` owns its private Tokio runtime rather than relying on
a frontend runtime. Keep original integration tests with the shared capability,
without a reverse config-to-core dependency. Tauri-specific adapters remain in
the host, outside neutral crates.

Domain configuration must not import a GUI package merely for a wire enum. Put
such a type in an appropriate neutral owner and preserve its schema and public
type identity. Move implementations and migrate callers together; do not keep
old-path wrappers solely to avoid updating imports.

The migration is incremental. A shared runtime builder does not mean the entire
`NyanpasuClient` or profile transaction graph has already moved out of Tauri.
Record remaining ownership in the extraction plan and move complete call paths,
preserving commit/vote/compensation and owner lifecycle semantics.

Run `deno task lint:backend-boundaries` after changing backend package direction.
It inspects resolved production/build dependencies for all targets, including
transitive GUI leaks. Its source is in the architecture-ledger script category;
`deno task test:backend-boundaries` runs only its policy tests. Run relevant Rust
behavior tests and desktop consumer checks as well: a dependency gate does not
prove behavior.
