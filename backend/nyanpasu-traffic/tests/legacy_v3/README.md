# Schema-v3 migration oracle

`expected.json` is the canonical, compact JSON produced by the **old implementation**,
not a result blessed from the tsdb implementation. It contains 16 reports, 208 complete
usage-page walks (264 pages), 208 keyed-usage results, two complete closed-connection
walks (2,048 and 387 connections, without cursors), and a summary. Object keys are
sorted with `Value::sort_all_objects`, so the bytes do not depend on whether
serde_json's `preserve_order` feature is enabled; array order and every result field
are preserved.
The fixed query clock is `fixture::NOW = 1700006400000`.

The helper in `fixture.rs` writes schema-v3 redb tables directly, with the old JSON
encoding. There are 216 combinations, 1,296 minute and 1,296 hour rows (including
unpruned rows several days old), 2,048 closed connections (four per close time),
seven active connections, and session metadata. Optional fields equal to v3 serde
defaults are omitted in selected rows. The same helper writes the file for both
the migration test and the external oracle program.

## Provenance and regeneration

The baseline is origin/main commit `df7d3094b31a87f9cc30e8411d7cd067f991f911`, whose
`backend/nyanpasu-traffic/src/redb.rs` is blob `8e7b3f6724491483282c3b319a2324a0b8368cd2`.
`oracle.rs` is the program that produced `expected.json` against that crate. It is not
built by this crate. To regenerate, without flushing or pruning the fixture:

1. Export the fixture from this crate (from `backend/`):

   ```sh
   NYANPASU_LEGACY_FIXTURE=/path/to/fresh/v3.redb cargo test -p nyanpasu-traffic --test legacy_v3 export_v3_fixture -- --ignored
   ```

2. Check out the baseline commit elsewhere (for example `git worktree add`). Next to its
   `backend/nyanpasu-traffic`, create a binary package with `oracle.rs` as
   `src/main.rs` and a copy of `fixture.rs` as `src/fixture.rs`. It depends on the old
   crate by path, plus `serde_json = "1"`.
3. Run it with the exported file and the output path:

   ```sh
   cargo run -- /path/to/fresh/v3.redb /path/to/expected.json
   ```

The program opens the file with the old `RedbTrafficStore`, restores the session from
`load()`, and for every `fixture::requests()` entry builds the old query rows (stored
usage with rate `None`, plus `Session::active_rows` for scope All, filtered with
`filter_rows`). It records `report`, every `usage_page` walk with limit 7 for every
`Dimension::ALL`, and `usage_by_keys` over the walked keys plus `"absent"` and `""`.
It walks `closed_connections` for every `fixture::selections()` entry with limit 137,
keeping only the connections, and records `Session::summary`. The result is written
with `serde_json::to_vec`.

The old implementation applied every filter to every ranking and usage page. A ranking
or usage page now ignores the filter on its own dimension, so the test leaves the
rankings, usage-page walks and keyed usage of each request's filtered dimensions out of
both sides before comparing them; the query tests cover those.

Do not regenerate expected data by calling the new store's query methods.
`.prettierignore` excludes `expected.json`, because the test compares it byte for byte.
