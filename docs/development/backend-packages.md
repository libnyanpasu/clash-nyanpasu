# Backend package boundaries

Tauri is the desktop host and GUI/transport adapter. Shared application behavior
and concrete infrastructure implementations live in separate neutral crates.
See the [extraction plan](../design/application-platform-extraction.md) for the
remaining migration graph; this guide defines the current dependency rules.

| Package             | Owns                                                                                             | Must not depend on                                  |
| ------------------- | ------------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| `nyanpasu-config`   | Domain models, profile validation, pure runtime executor and its ports                           | Tauri or egui/eframe                                |
| `nyanpasu-core`     | NyanpasuClient, use cases, typed actor clients, workflows, consumed ports and state transactions | Platform implementation crate, Tauri or egui/eframe |
| `nyanpasu-platform` | Concrete script, filesystem, process and OS adapters                                             | Tauri host or egui/eframe                           |
| `backend/tauri`     | Desktop composition root, concrete Tauri adapters, GUI lifecycle and IPC                         | No reverse dependency from neutral crates           |

The core crate receives dependencies through constructors, function
arguments or actor startup parameters. Platform can depend on core to
implement its ports. Core must not import platform in production. Keep
cross-layer tests in platform or host tests rather than creating a dependency
cycle. Tauri-specific adapters remain in the host, outside neutral platform.

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
`deno task test:scripts` covers its policy tests. Run relevant Rust behavior tests
and desktop consumer checks as well: a dependency gate does not prove behavior.

## Consolidated runtime baseline

The runtime builder, builtin transforms and script descriptor types originally
extracted into `nyanpasu-application` now live in `nyanpasu-core::enhance`.
Platform and desktop consumers import core directly; the application crate and
its workspace/dependency entries have been removed. Continue the core extraction
PRs from this baseline without introducing a second application layer. The full
facade and its dependent actor/workflow graph still require their upstream
migration; this consolidation does not claim that those capabilities have moved.
