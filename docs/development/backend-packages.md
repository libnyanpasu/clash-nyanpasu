# Backend package boundaries

Tauri is the desktop host and GUI/transport adapter. Shared application behavior belongs in `nyanpasu-core`, independent of the host.
Concrete infrastructure stays behind narrow ports and explicit construction.
See the [extraction plan](../design/application-platform-extraction.md) for the
remaining migration graph; this guide defines the current dependency rules.

| Package                | Owns                                                                             | Must not depend on                                  |
| ---------------------- | -------------------------------------------------------------------------------- | --------------------------------------------------- |
| `nyanpasu-config`      | Domain models, profile validation, pure runtime executor and its ports           | Tauri or egui/eframe                                |
| `nyanpasu-core`        | Shared use cases, typed clients, workflows, ports and persistence primitives     | Tauri or egui/eframe                                |
| `nyanpasu-application` | Existing runtime/builtin slice awaiting migration into core; no new capabilities | Platform implementation crate, Tauri or egui/eframe |
| `nyanpasu-platform`    | Concrete script, filesystem, process and OS adapters outside core capabilities   | Tauri host or egui/eframe                           |
| `backend/tauri`        | Desktop composition root, concrete Tauri adapters, GUI lifecycle and IPC         | No reverse dependency from neutral crates           |

`nyanpasu-application` is a transitional owner, not the target application layer.
Move its existing implementation, original tests and all consumers into core in
complete migration units, then delete the package. Do not add new capabilities to
it or duplicate its existing implementation in core while callers still use it.

Shared application code receives dependencies through constructors, function
arguments or actor startup parameters. Ports belong to the consuming core
capability. Concrete non-GUI adapters may be capability-local in core when hosts
need to construct that capability directly: `nyanpasu-core::device` owns its
subscription device contract, pure header rule and lazy OS source, with OS IO
isolated in `device::os`. This does not make pure services perform hidden IO or
permit process-global caches. Other existing platform adapters stay in platform;
platform may depend on the consuming neutral owner to implement its ports.
Application must not import platform in production. Keep cross-layer tests with
the implementation or host rather than creating a dependency cycle.
Tauri-specific adapters remain in the host, outside neutral crates.

Domain configuration must not import a GUI package merely for a wire enum. Put
such a type in an appropriate neutral owner and preserve its schema and public
type identity. Move implementations and migrate callers together; do not keep
old-path wrappers solely to avoid updating imports.

The migration is incremental. A shared runtime builder does not mean the entire
`NyanpasuClient` or profile transaction graph has already moved out of Tauri.
Record remaining ownership and the pending application-package removal in the
extraction plan and move complete call paths,
preserving commit/vote/compensation and owner lifecycle semantics.

Run `deno task lint:backend-boundaries` after changing backend package direction.
It inspects resolved production/build dependencies for all targets, including
transitive GUI leaks. Its source is in the architecture-ledger script category;
`deno task test:scripts` covers its policy tests. Run relevant Rust behavior tests
and desktop consumer checks as well: a dependency gate does not prove behavior.
