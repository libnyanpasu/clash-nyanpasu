# Domain errors

Status: draft, 2026-10-30. Scope: the Rust backend (`backend/`, the
`nyanpasu-runtime` submodule and `nyanpasu-utils`).

This document answers recurring design questions about error types: what a domain
error is, what shape an error type takes, and when `?` may convert an error
implicitly. The binding rules stay in the [Rust code guide](../development/rust.md#7-errors-panics-and-assertions);
this document gives the reasoning, the sources it rests on, and the current state of
the code.

## 1. Why this matters here

Runtime v2.0.2 reported every journal failure as `nyanpasu_jobs::Error::Journal(String)`.
When `jobs.redb` was damaged, the application could not tell corruption from a held
lock or a permission problem, so it could neither rebuild the journal nor tell the
user what to do; startup stopped until the user deleted the file by hand. Fixing it
took typed journal errors in the runtime (nyanpasu-runtime#463), a shared redb
classification in nyanpasu-utils (#212) and a rebuild path in the application
(#6069).

The failure was not a missing feature. It was an error shape: the information that
the caller needed existed at the point of failure and was thrown away by
`to_string()`. Most rules below exist to keep that from happening again.

## 2. Definitions

### 2.1 Three kinds of failure

Scott Wlaschin (_Domain Modeling Made Functional_, quoted in "Against
Railway-Oriented Programming") separates three kinds of failure. Domain errors are
"errors that are to be expected as part of the business process, and therefore must
be included in the design of the domain"; panics "leave the system in an unknown
state" or come from "programmer oversight"; infrastructure errors are "expected as
part of the architecture, but are not part of any business process", and
"sometimes handling these should be modeled as part of the domain, and sometimes
they can be treated as panics". This repository uses the same split:

| Kind                   | Meaning                                                                                                                                                                  | Mechanism here                                                                             |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| Domain error           | An expected outcome of the operation that the caller must handle by a business rule: a revision conflict, a profile that does not exist, a core that is already running. | A variant of the operation's error type. Part of the signature.                            |
| Infrastructure failure | The environment refused: IO, network, IPC, a database, an external process.                                                                                              | A `source` inside the domain variant of the step that failed, keeping the underlying type. |
| Bug                    | A violated invariant: the program is wrong.                                                                                                                              | A panic ([Rust code guide §7.1](../development/rust.md#71-choosing-the-mechanism)).        |

Wlaschin and Vladimir Khorikov would let an infrastructure failure that nobody can
handle propagate as an exception. Rust has no recoverable exception, and a desktop
application that manages files, databases and processes on the user's machine
handles most of these failures: it retries a held database, rebuilds a corrupt one,
and reports a permission problem differently from a busy one. So here an
infrastructure failure is modelled in the type as the `source` of the step that
failed, and the classification the caller needs is exposed as a method (§4.5),
never by flattening the cause.

### 2.2 Two audiences

Every error has two readers:

- **Code** that branches on it: retries, rebuilds, falls back, maps it to an RPC
  code. It needs the variant and structured fields.
- **A person** reading a log or a dialog. They need the `Display` text of each layer
  of the `source()` chain, from the operation that failed down to the OS error.

Luca Palmieri puts it as "Errors serve two main purposes: Control flow [...];
Reporting", where control flow needs information "accessible to a **machine**" and
reports "are primarily consumed by **humans**". An error type serves the first
reader through its variants and the second through `Display` plus `source()`. Neither substitutes for the other: a message string
cannot be matched, and a variant without context cannot be acted on.

## 3. What the libraries and `std` say

### 3.1 snafu

From the snafu 0.9.2 user's guide (`snafu::guide::philosophy`):

> It should be easy to bin one underlying error type (such as `io::Error`) into
> multiple domain-specific errors while optionally adding contextual information.

> Each module should have one (or more!) error types that are scoped to that module,
> reducing the need to deal with unrelated errors when matching and increasing
> cohesiveness of a single error type.

The guide's comparison chapter shows the consequence: one `ParseIntError` becomes
`AreaCodeInvalid { value, source }` or `PhoneExchangeInvalid { value, source }`,
depending on which step parsed it, and states "You do not have to have a one-to-one
relationship between an underlying error and an error variant".

On skipping the context selector (`#[snafu(context(false))]`, which lets `?` convert
the source directly):

> Sometimes, an underlying error can only occur in exactly one context and there's
> no additional information that can be provided to the caller. [...] Please think
> about your end users before making liberal use of this feature. Adding context to
> an error is often what distinguishes an actionable error from a frustrating one.

`#[snafu(transparent)]` implies `context(false)` and also delegates `Display` and
`source()` to the wrapped error. The guide's example uses it for a validation error
type (`GroupIdError`) reused by two operations, where "there's nothing useful to say
in addition to the validation error message".

For public APIs the guide (`snafu::guide::opaque`) recommends an opaque newtype,
`pub struct Error(InnerError)`, so that consumers cannot rely on variants and fields
that are free to change.

`snafu::Report` prints an error and its whole `source()` chain. When a layer's
`Display` repeats its source's text, the report removes the repetition and marks the
line: "Some redundant information has been removed from the lines marked with *."

### 3.2 thiserror

thiserror 2 generates `From` for a variant with `#[from]`, and that variant "must not
contain any other fields beyond the source error". So a `#[from]` variant can only
be named after the source type, and the one conversion is used at every `?` that
produces that type, whichever step it came from.

`#[error(transparent)]` is described as "appropriate for enums that need an
'anything else' variant", with the example `Other(#[from] anyhow::Error)`, and for an
opaque public newtype over a private representation.

Its README positions the crate as: "Use thiserror if you care about designing your
own dedicated error type(s) so that the caller receives exactly the information that
you choose in the event of failure."

thiserror can express step-named variants with context: a named `source` field plus
`map_err(|source| Error::ReadConfig { path, source })`. Nothing forces the
`#[from]` shape; the derive just makes it the shortest path, while snafu makes the
context selector the shortest path.

### 3.3 `std::error::Error`

The `std::error::Error` documentation: "the underlying error should be either
returned by the outer error's `Error::source()`, or rendered by the outer error's
`Display` implementation, but not both." The Error Handling Project Group (Inside
Rust blog, 2021-07-01) states the same rule and expects the default to be `source()`,
with reporting "the primary role" and reacting via `downcast` second. Both also note
that messages are "concise lowercase sentences without trailing punctuation".

snafu agrees: "Printing an error via `Display` will only show the top-level error
message without the underlying sources"; `Report` prints the chain.

### 3.4 Community writing

Where the sources agree:

- **Wrap by context, not by type.** snafu's philosophy, Sabrina Jewson ("error types
  should be located near to their unit of fallibility"), GreptimeDB and Nick Cameron
  all say the step that failed belongs in the error. GreptimeDB chose snafu because
  thiserror's `From`-based variants mean "you cannot define two error variants from
  the same source type [...] the context is blurred in type".
- **Implicit `From` loses meaning.** Sabrina: a `From<io::Error>` "would implicitly
  add meaning that one failed during the process of reading the file [...] rendering
  `From` inappropriate". Cameron calls nested `From` variants "_very_ overused, to
  the point where you should consider it an anti-pattern".
- **No catch-all `errors` module.** Sabrina compares it to "a drawer labelled
  'medium-sized and flat'"; Cameron: "Keep your errors in the module they serve."
- **Opaque types at public boundaries.** snafu's opaque guide, thiserror's
  `transparent` newtype, and Andrew Gallant's `ParseIntError` example.
- **Do not log and propagate.** Palmieri: "If your function is propagating the error
  upstream (e.g. using the `?` operator), it should **not** log the error."

Where they disagree:

- **Granularity.** snafu says one or more types per module; Sabrina says per unit of
  fallibility; Dmitrii Aleksandrov says "an enum per function, instead of a global
  `Error` enum", and counts private workspace libraries as application code. Andrew
  Gallant went the other way in Jiff: "finer grained error types proved difficult in
  the face of composition", and he found "nobody really cares. Most of the time, for
  errors, you just want to print them". He had started "converting `RangeError`s into
  `String` values in order to combine them" — the flattening §4.3 forbids.
- **`From` ergonomics.** Gallant's 2015 guide recommends `From` impls so that `?` is
  ergonomic, and thiserror's `#[from]` exists for that; snafu, GreptimeDB, Cameron
  and Sabrina push back.
- **Diagnostics in typed errors.** Wlaschin keeps domain errors free of diagnostics
  ("using `Result` as a glorified `bool`"). GreptimeDB puts a `snafu::Location` in
  every error to build a "virtual user stack" at about 100 KB of overhead, instead of
  over 700 MB of debug info for system backtraces.

This repository sides with fine-grained types (§4.1). Jiff's callers mostly print
its errors; ours branch on them — §1 is a case where the caller had to tell
corruption from a held lock, and a coarse type that flattened the cause is what
prevented it. Where many types need the same decision, a shared classification
(§4.5) gives the composition Gallant wanted without merging the types.

## 4. Rules for this repository

Each rule says what to do, why, and the exception, if any.

### 4.1 One error type per module or operation

A public operation returns an error type that contains only the failures that
operation can produce. A caller matching on it never sees variants from unrelated
operations.

- Do not create one crate-wide `Error` enum that every function returns.
- A module may have several error types, one per operation group, as
  `nyanpasu-local-channel` does with `HandshakeError`, `SendError` and `RecvError`.
- A failure shared by several modules, such as a codec or budget error, is defined
  once and appears as the `source` of each caller's own step variant.

### 4.2 Name variants after the step that failed

A variant names the step and carries that step's data; the underlying error is its
`source`.

```rust
#[derive(Debug, Snafu)]
pub enum LoadProfileError {
    #[snafu(display("could not read profile {uid} at {path}"))]
    ReadContent { uid: ProfileId, path: Utf8PathBuf, source: io::Error },
    #[snafu(display("profile {uid} is not valid YAML"))]
    ParseContent { uid: ProfileId, source: serde_yaml_ng::Error },
}
```

- The same source type may appear in several variants; that is the point.
- A variant named after its source type (`Io`, `Yaml`, `Json`, `Redb`) is a smell:
  it says what failed, not where. It is acceptable only when the error type is
  already scoped to a single step, as in `SendError::Io` ("could not write the
  message").
- Do not encode the step as a string field (`Storage { op: &'static str }`). A string
  cannot be matched exhaustively, and a typo is silent.

### 4.3 Keep the cause typed

Do not turn an error into a string to store it in another error.

- No `Variant(String)` or `detail: String` that holds a formatted error. This is the
  shape that caused §1.
- `map_err(|e| e.to_string())` is allowed only at a boundary that must produce text
  (a CLI exit message, a third-party callback that takes a string), never between two
  Rust layers.
- If the error must be `Clone` or `PartialEq` (it is broadcast, cached, or compared in
  tests), keep the source in an `Arc`, or compare by variant or `kind()` in tests.
  Do not derive `Clone` by flattening the cause.
- A `String` field is fine when the data really is text: a reason the peer sent, a
  log tail from the core, a user-entered value.

### 4.4 `Display` describes this layer only

`Display` says what this layer was doing and with which data. It does not repeat
`{source}`; a reporter walks the `source()` chain.

- Log an error with `snafu::Report::from_error(&error)` (or by walking `source()`),
  not with `%error`, so that the cause is printed.
- Log where the error is handled, not at every layer that propagates it with `?`.
- `#[snafu(transparent)]` / `#[error(transparent)]` delegate `Display` to the source,
  which is consistent with this rule.

The RPC layer does not follow this yet: `RpcError::application_ref` sends only the top
error's `to_string()` as `message`, so today a variant that drops `{source}` from its
`Display` also drops the cause from the dialog. Render the chain in `RpcError` before
removing `{source}` from existing messages (see §6).

### 4.5 Classification is a method, not a variant

When callers need the same decision for many error types (retry, rebuild, report as
permission denied), expose it as a method that delegates to the source, instead of
adding variants or matching messages:

- `nyanpasu_utils::io::classify(&io::Error) -> IoFailure`
- `nyanpasu_utils::redb_ext::RedbErrorExt::failure()` and
  `find_failure(&dyn Error)`, which walks a `source()` chain
- `nyanpasu_tsdb::StorageError::kind()`
- `nyanpasu_core_manager::CoreError::kind` and `retryable`, read by `RpcError`

The policy (what to do for each class) stays with the owner of the operation; only
the classification is shared.

### 4.6 Implicit conversion only for a unique, self-explaining source

`#[from]`, `#[snafu(context(false))]` and `#[snafu(transparent)]` make `?` convert
without context. Use them only when both hold:

1. the source type can come from exactly one step of this operation, and
2. the source already says everything the caller needs.

That fits a dedicated domain error reused as-is, such as the `NotReady` and
`ProfileRevisionError` sources under `context(false)` in `tauri/src/state`, or the
`SnapshotBuildError` under `transparent` in `nyanpasu-config`. It never fits a general
type that many steps produce: `io::Error`, `serde_*::Error`, `redb::*Error`,
`reqwest::Error`, `anyhow::Error`.

### 4.7 No catch-all variants

Do not add `Custom(String)`, `Other(#[from] anyhow::Error)`, `Unknown` or `Internal {
message }` as a place for failures that have no variant yet. Add the variant. A
caller cannot handle a catch-all, and it grows. Two exceptions:

- `anyhow::Error` at the composition root, in startup, binaries and tests
  ([Rust code guide §7.2](../development/rust.md#72-error-types)).
- An error from a third party whose type carries no structure is kept as its own
  type in the step variant, not converted to text.

### 4.8 Public errors across crates

- Mark a public error enum `#[non_exhaustive]` when its variants may grow.
- When the variants are an implementation detail, publish an opaque newtype over a
  private enum (`snafu::guide::opaque`) and expose accessors, such as `kind()`, for
  what callers may branch on.

### 4.9 Errors across RPC

- A domain error that the frontend acts on is `Serialize` and travels in
  `RpcError::domain_error`; fields that are not data (the `source`) are
  `#[serde(skip)]`.
- `RpcError` finds known domain errors by walking the `source()` chain with
  `downcast_ref`; wrapping a domain error in another error keeps it findable, but
  formatting it into a string does not.

### 4.10 Tests

Match the variant and its fields (`assert!(matches!(error, LoadProfileError::ReadContent { .. }))`
or a `kind()`), not the message text. A test that compares messages breaks on wording
and passes when the variant is wrong.

## 5. Choosing snafu or thiserror

The problem is the shape, not the derive crate: a thiserror enum with step-named
variants and `map_err` context is fine, and a snafu enum of `context(false)`
wrappers is not. That said:

- New modules use snafu ([Rust code guide §7.2](../development/rust.md#72-error-types)),
  because its context selectors make the step-named shape the shortest code, and
  `ensure!`, `OptionExt::context` and `Report` cover the common cases.
- Do not switch a module's derive crate in an unrelated change. When a module is
  reshaped for these rules anyway, moving it to snafu in the same change is fine.

## 6. Current deviations (2026-10-30)

Counts come from `grep` over `backend/` and are approximate. The runtime numbers are
for the submodule pin in the main checkout (v2.0.2), before nyanpasu-runtime#463.

| Where                                                            | Shape                                                                                                                                                                                                                                                                                                                             | Rule          |
| ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------- |
| `tauri/src/client/error.rs` `ClientError`                        | Crate-wide enum of ten `#[error(transparent)] #[from]` wrappers (`Io`, `SerdeYaml`, `SerdeJson`, `Anyhow`, ...) plus `Custom(String)` and `From<String>`/`From<&str>`. Only four wrappers map to a typed `IpcErrorKind`; the rest become `Unknown`.                                                                               | 4.1, 4.6, 4.7 |
| `nyanpasu-core-manager/src/error.rs` `Error`                     | One enum for the whole manager; `#[from]` for `io::Error`, `serde_yaml_ng::Error`, `clash_api::Error`, `ProcessError`; flattened causes in `ConfigCheckFailed(String)`, `ApplyFailed(String)`, `StopUnconfirmed(String)`, `CoreVersionProbeFailed { detail: String }`, `ApplyRollbackFailed { apply: String, rollback: String }`. | 4.1, 4.3, 4.6 |
| `nyanpasu-jobs/src/model.rs` `Error`                             | `Journal(String)`, `AdmissionFailed(String)`, `OutputUnavailable(String)`; derives `Clone`/`PartialEq`, which forces the flattening. Typed journal errors are in nyanpasu-runtime#463.                                                                                                                                            | 4.3           |
| `nyanpasu-config/src/runtime/snapshot.rs` `SnapshotPersistError` | `#[from]` for `io::Error` and the `rmp_serde` encode/decode errors, so a failure does not say which file or step.                                                                                                                                                                                                                 | 4.2, 4.6      |
| snafu `display(".. {source}")`                                   | 33 variants (tauri 14, nyanpasu-tsdb 9, enable-loopback 8, others 2). Compensates for the RPC layer printing only the top message.                                                                                                                                                                                                | 4.4           |
| `map_err(\|e\| ..to_string())` outside tests                     | About 93 sites (tauri 21, nyanpasu-diagnose 20, nyanpasu-platform 18, nyanpasu-utils 17, others 17); many are Tauri command boundaries, which are allowed.                                                                                                                                                                        | 4.3           |

Already in the target shape, and good references: `nyanpasu-local-channel/src/error.rs`
(per-operation types), `nyanpasu-tsdb` (step variants with `kind()`), and the
`tauri/src/state` profile and config errors (snafu, serializable, step-named).

Migration follows the [Rust code guide](../development/rust.md) policy for migration
debt: fix a type when a change already touches it, and do not reshape unrelated
modules in passing.

## 7. Checklist for a new error type

1. Which operation does this type belong to? Does every variant come from that
   operation?
2. Is each variant named after a step, with that step's data?
3. Is every cause kept as a typed `source`, not a string?
4. Does any `#[from]`, `context(false)` or `transparent` wrap a general type such as
   `io::Error` or a serde error? If so, replace it with a step variant.
5. Does `Display` describe only this layer?
6. Do callers need a classification that already exists (`io::classify`,
   `RedbErrorExt::failure`, a `kind()`)? Delegate to it instead of matching messages.
7. Is there a catch-all variant? Replace it with real variants.
8. If it crosses RPC: is it `Serialize`, with `source` skipped?
9. Do tests match variants rather than messages?

## 8. Open questions

- **Chain rendering in `RpcError`.** Should `message` carry the whole chain (as
  `snafu::Report` renders it) or only the top layer, with the chain in a separate
  field? This must be settled before §4.4 can apply to the existing `{source}`
  messages.
- **`ClientError`.** Replace it with per-domain errors at the command boundary, or
  keep it as the RPC adapter's error and remove only `Custom`, `Anyhow` and the
  general-type wrappers?
- **Runtime crates.** The runtime is a separate repository with its own review; these
  rules apply there only once that repository adopts them.
- **`snafu::Location`.** GreptimeDB records the construction site in every error. It
  only points at the right place when the error is built by a context selector, not
  by `?` through `context(false)`, so it fits the rules here; whether the log value
  justifies an extra field in every variant is undecided.

## 9. Sources

Primary, read from the published crate sources:

- snafu 0.9.2 user's guide: `snafu::guide::philosophy`, `snafu::guide::opaque`,
  `snafu::guide::comparison::failure`, and the `Snafu` derive documentation sections
  "Disabling the context selector", "Delegating to the underlying error" and
  "Controlling stringly-typed errors". <https://docs.rs/snafu/0.9.2/snafu/guide/index.html>
- snafu 0.9.2 `Report` and `CleanedErrorText`. <https://docs.rs/snafu/0.9.2/snafu/struct.Report.html>
- snafu `Location`. <https://docs.rs/snafu/0.9.2/snafu/type.Location.html>
- thiserror 2 README, including "Comparison to anyhow". <https://docs.rs/thiserror/2/thiserror/>
- `std::error::Error`. <https://doc.rust-lang.org/std/error/trait.Error.html>
- Rust Error Handling Project Group, "What the Error Handling Project Group is Working
  Towards", Inside Rust blog, 2021-07-01.
  <https://blog.rust-lang.org/inside-rust/2021/07/01/What-the-error-handling-project-group-is-working-towards/>

Community:

- Sabrina Jewson, "Modular Errors in Rust", 2023-04-08. <https://sabrinajewson.org/blog/errors>
- Andrew Gallant, "Error Handling in Rust", 2015, updated 2020.
  <https://blog.burntsushi.net/rust-error-handling/>; on Jiff's single error type:
  <https://github.com/BurntSushi/jiff/issues/8> and <https://docs.rs/jiff/latest/jiff/struct.Error.html>
- Luca Palmieri, "Error Handling In Rust - A Deep Dive".
  <https://www.lpalmieri.com/posts/error-handling-rust/>
- Ruihang Xia (GreptimeDB), "Error Handling for Large Rust Projects", 2024-05-07.
  <https://greptime.com/blogs/2024-05-07-error-rust>
- Nick Cameron, "Error type design". The book's index warns that parts are out of
  date and its author has changed his mind on some of the second half, which includes
  this chapter. <https://nrc.github.io/error-docs/error-design/error-type-design.html>
- Dmitrii Aleksandrov, "Designing Error Types in Rust Applications".
  <https://home.expurple.me/posts/designing-error-types-in-rust-applications/>

Outside Rust:

- Scott Wlaschin, "Against Railway-Oriented Programming", 2019-12-20, quoting
  _Domain Modeling Made Functional_.
  <https://fsharpforfunandprofit.com/posts/against-railway-oriented-programming/>
- Vladimir Khorikov, "Error handling: Exception or Result?".
  <https://enterprisecraftsmanship.com/posts/error-handling-exception-or-result/>

The crate documentation was read from the published sources of snafu 0.9.2 and
thiserror 2.0.21. The other pages were fetched on 2026-10-30. Wlaschin's book itself
was not read.
