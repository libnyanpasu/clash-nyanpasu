# Diagnosis design

- `src/lib.rs`: `Step`, `Event`, `Problem`, and an ordered list of step closures.
  Each closure receives the report, optional archive, and a fraction callback.
  A failed step is recorded and collection continues. A disconnected consumer
  never cancels owner-started work. The host runs this synchronous I/O on a
  blocking thread; the crash child uses its own thread.
- `src/host.rs`: CPU grouping, RAM and GPU facts, with no process enumeration.
- `src/report.rs`: typed facts, the generated `PageCapabilities` RPC DTO, and
  ordered System, Device, Core, WebView, Page and Build Info sections. Diagnostics
  problems and truncation notes follow them. The host supplies any panic prefix.
- `src/archive.rs`: redacted app logs, host-exported core text, final `report.txt`,
  atomic temporary-file publication and retention of the newest five zips.

App logs must match `clash-nyanpasu_*.log` and have been modified within three days.
Both log steps cap uncompressed output at 64 MiB, keeping the newest complete
content and recording truncation. Progress for app files counts source bytes read;
core archive writing counts exported bytes. Core-store scanning is host-owned.
`nyanpasu-config::redact` owns the unchanged credential rules and their text form;
this is credential redaction, not the future levelled privacy policy.

The crate depends on config, sysinfo, gpu-info and archive/serialization libraries.
It does not depend on core, platform, Tauri or Slint. It takes log paths explicitly;
it does not discover a user's application identity or configuration.

## Implementation decisions relative to the plan

- Archive names gain a numeric suffix only on a same-second collision, because
  concurrent manual reports are allowed. Normal names keep the planned timestamp.
- The archive layer takes `Path` values rather than depending on `nyanpasu-paths`;
  the desktop composition derives those values from its injected `PathResolver`.
- There is no application-config loader in `nyanpasu-config`. The host uses its
  existing host `ApplicationFormat` (a `StampedYamlFormat`) with `NyanpasuAppConfig`, the same
  typed persistence format used by `ApplicationClient`, instead of parsing YAML
  fields separately.
- Core-log pages contain 4096-byte previews. The host uses a single store export
  operation, serialized by the running actor, with one read transaction/decoder
  per shard. A crash-only preserving opener reads the same shards after the parent
  exits. Shutdown now flushes instead of deleting those shards; the existing
  next-session opener still removes them. Without this lifecycle change a
  gracefully shutting-down panic parent would erase the evidence before export.
- Reveal uses small platform commands (`explorer /select`, `open -R`, and directory
  opening on Linux). The existing `open` dependency handles Linux; no additional
  opener library is needed.

The host's opt-in `utils::collect::tests::measure_diagnose` takes explicitly staged
config/data roots through `NYANPASU_DIAGNOSE_CONFIG` and `NYANPASU_DIAGNOSE_DATA`.
`core::logs::tests::diagnostic_export_large_store_measurement` compares bulk export
with cursor pages plus full-record detail reads on a temporary 96 MiB payload
store. Both are ignored during ordinary tests and print timings with `--nocapture`.
