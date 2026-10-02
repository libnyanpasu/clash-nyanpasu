# nyanpasu-geodata design

## Goal

Answer "where is this address / what is this host" from the databases the
mihomo core uses, with low resident memory and sub-microsecond lookups. The
crate is a pure service: bytes in, immutable index out. An index can also be
stored as bytes and reopened from a read-only map of them. Discovering the
core's home directory, reloading after a geo update, storing indexes and
caching per connection belong to the caller.

## Layout

Dependencies point one way, `index` → `parser` → `collection`:

- `collection/`: the data structures. `scratch.rs` holds build buffers in
  anonymous memory, `image.rs` the byte image every index lives in,
  `range_table.rs` the IP range table and `tags.rs` interned tag names and
  sets.
- `parser/`: the database formats, each read into collections. `proto.rs` is
  the protobuf field reader; `mmdb.rs` and `geoip_dat.rs` compile their format
  into a range table, and `geosite_dat.rs` collects GeoSite rules by kind.
- `index/`: the public indexes `IpIndex`, `AsnIndex` and `SiteIndex`, which
  decode records the way the core does and answer lookups.
- `files.rs` finds the core's files and reads them; `error.rs` holds
  `GeoError`.

## Why compile instead of reading the files in place

- **No mmap of the core's files.** mihomo rewrites `Country.mmdb` in place
  (`os.WriteFile` after closing its own mapping, `component/updater/update_geo.go`).
  A mapping held by this process would make that write fail on Windows and could
  read torn data or raise `SIGBUS` on Unix.
- **Smaller and faster than keeping the file.** An MMDB search tree spends
  6–7 bytes per node; flattening it into sorted address ranges and merging
  neighbours with the same record roughly halves country databases and keeps a
  city database at country size (see benchmarks).
- **One lookup structure for every IP source.** MMDB country, MMDB ASN and
  `GeoIP.dat` all compile into the same range table.

## Memory: the build leaves nothing behind

The system allocators keep freed heap pages for the process's later
allocations instead of returning them to the OS. Measured on macOS:

- freeing a 121 MiB `Vec` leaves `phys_footprint` unchanged, even after
  `malloc_zone_pressure_relief`;
- mimalloc retains as much or more.

A build that ran on the heap would therefore leave its peak, file bytes
included, as the process footprint: 136 MiB for a 121 MiB city database whose
index is 5 MiB. Anonymous mappings, by contrast, go back to the OS when they are
unmapped, whatever the allocator. Hence:

- `read_source` copies a file into anonymous memory. Being a copy, it never
  maps the core's file.
- Build buffers that scale with the input live in anonymous memory too
  (`collection/scratch.rs`): range-table step lists, MMDB record caches and
  the tree check, `GeoIP.dat` sweep events, GeoSite keys, postings and FST
  output.
  `ScratchVec` grows by remapping, so the old mapping is returned at once, and
  `ScratchMap` is an open-addressing table on top of it.
- The index itself is copied once into an image in anonymous memory of its
  exact size (see below). Only GeoSite's keyword automaton and regex set live
  on the heap.
- ASN records are keyed by data offset in a `ScratchMap` rather than
  deduplicated by content in heap hash maps.

What remains on the heap during a build is small and bounded (tag and list
interners, regex compilation), with one exception: the `fst` builder's node
registry allocates many small blocks. The registry itself is bounded, but its
freed blocks fragment the heap, so a GeoSite build still leaves some freed
heap behind (see benchmarks). `tests/build_heap.rs` guards against large build
buffers returning to the heap: what a build holds on the heap beyond its
result must stay small, and for GeoSite must not grow with the input. It
measures heap peaks, so it cannot see fragmentation.

## IP range table (`collection/range_table.rs`)

Sorted step lists: `starts[i]` is the first address mapped to `ids[i]`, so a
lookup is one `partition_point`.

- IPv4: `u32` starts.
- IPv6: `u64` starts over /64 blocks. A block containing any boundary finer
  than /64 is marked `FINE` and continues in a small `u128` table. Real
  databases have no such boundary except `::1/128`-style entries.
- Addresses under `::/96` are looked up in the IPv4 table, the way an MMDB tree
  nests its IPv4 subtree; IPv4-mapped addresses are canonicalised first.
- Ids are `u16` (tag sets) or `u32` (ASN records); the two largest values mark
  gaps and fine blocks.

## Index images (`collection/image.rs`)

Every index is one byte image:

- a header: magic, format version, index type, section count and the CRC-32
  of everything after the header;
- a section table of starts and lengths;
- sections of plain values in native byte order, each aligned to 16 bytes.

A build writes the image straight into anonymous memory, and lookups slice its
sections in place; that access costs a few nanoseconds per lookup over owned
arrays. `as_bytes` returns the image and `from_bytes` opens one, typically a
read-only map of a file it was written to. Opening checks the header, the
alignment, each section's shape and the checksum, then that every string is
UTF-8 and every step list starts at address zero. The checksum detects damage,
not tampering: an image is trusted like the code that wrote it. Any error
means the caller rebuilds from the source.

GeoSite stores its FST, postings and pattern strings. Aho-Corasick and
`regex-automata` have no stable serialized form, so the keyword automaton and
the regex set are rebuilt on open: about 9 ms and 1.3 MiB of heap for the
bundled file.

Pages mapped from a file are file-backed and clean. They count in the working
set and RSS but not in private bytes, `phys_footprint` or `RssAnon`, take no
commit charge, and can be dropped under memory pressure without a pagefile
write. A stored index is thus nearly free in the process's private memory;
see the benchmarks.

A caller caching images keys them by the source content, writes a new image to
a temporary file and renames it into place, and never rewrites a mapped file:
a mapping of a file changed underneath reads torn data, the reason this crate
never maps the core's own files. On Windows, opening the file without
`FILE_SHARE_WRITE` keeps other writers out while it is mapped.

## Sources

**MMDB (`parser/mmdb.rs`, `index/ip.rs`, `index/asn.rs`).** `maxminddb` walks
the networks, and a fixed 64 Ki-slot cache keyed by data offset decodes each record about once.
A city database with millions of records costs at most 768 KiB of anonymous memory.
IP tag sets are interned by content, so a cache eviction only repeats a decode.
ASN records are kept exactly once per data offset.
Before walking, `check_tree` rejects search trees in which a node other than the
IPv4 start has two parents: a crafted DAG would make the walk exponential, and
neither the iterator nor `Reader::verify` bounds it. Cycles need no check; the
iterator fails on the first path deeper than the address width. Every real
database tested has single-parent nodes plus the three standard aliases.

**`GeoIP.dat` (`parser/geoip_dat.rs`).** Per-code CIDR lists are inverted with
a sweep over open/close events, one address family at a time in a buffer sized by a
counting pass. Each range carries every code containing it, in file order.

**`GeoSite.dat` (`parser/geosite_dat.rs`, `index/site.rs`).**

- `Domain` and `Full` values are stored reversed in an FST. A lookup walks the
  host backwards once and collects postings at label boundaries.
- `Plain` keywords go through Aho-Corasick.
- `Regex` rules go through one `regex-automata` meta regex, configured as
  `regex::RegexSet` is. Patterns it rejects (look-around, for instance) are
  skipped and counted.
- Every lookup shares one regex search cache behind a mutex. `RegexSet` keeps a
  cache of about 0.8 MiB per concurrent caller and never frees it, so its
  memory followed the peak thread count; a shared cache keeps it at one, at the
  cost of serializing the regex step.
- Postings are `u32`: `FULL | list << 16 | attribute set`.
- `keep` limits the lists indexed.

## Parity with the core

- **Country codes.** Codes follow `mmdb.IPReader.LookupCode`:
  - MaxMind layout (`country.iso_code`) for any unrecognised `database_type`;
  - a bare string for `sing-geoip`;
  - a string or a list for `Meta-geoip0`.

  Codes are lowercased and repeats dropped. Records of an unexpected shape
  yield nothing, like the core's ignored decode errors.

- **ASN records.** Supported types are `GeoLite2-ASN`,
  `DBIP-ASN-Lite (compat=GeoLite2-ASN)` and `ipinfo generic_asn_free.mmdb`.
  Any other type is an error. `Asn::mihomo_label` reproduces `"<number> <org>"`.
- **`Tags::country`** returns a tag only when exactly one tag is a two-letter
  code, because databases mix in categories (`google`, `private`, `telegram`)
  and some MaxMind-typed files even put `GOOGLE` in `iso_code`.
- **GeoSite matching.** Matching follows the core's matchers: `Domain` matches
  the value and its subdomains, `Full` the value only, `Plain` is a substring
  match and `Regex` an unanchored search. Hosts are lowercased and lose a
  trailing dot. Attribute keys are compared case-insensitively.
- **`GeoIP.dat` entries.** `reverse_match` is ignored, as the core does.
- **Known deviations.**
  - The MMDB 6to4 (`2002::/16`) and Teredo (`2001::/32`) aliases are not
    mirrored, so those IPv6 addresses find nothing.
  - Entries of `GeoIP.dat` that repeat a code are merged. The core uses only
    the first such entry.
  - Unknown GeoSite rule types are skipped rather than failing the list.

## Robustness

Input files can come from any URL a subscription sets as `geox-url`, so they
are treated as untrusted:

- the protobuf reader is bounds-checked and allocation-free;
- every malformed input returns `GeoError`;
- tests cut fixtures at every length and feed random bytes, and none of them
  may panic.

Sizes are bounded where crafted input could otherwise blow up:

- one address carries at most 255 tags and one GeoSite rule at most 255
  attributes, since staggered overlaps would otherwise intern sets of
  quadratic total size;
- ids are capped by their width;
- the MMDB walk is bounded by `check_tree`.

What remains linear in the file size is the CPU spent compiling a GeoSite file
with very many large regexes. Each pattern is still bounded by the NFA size
limit `regex` uses (10 MiB).

## Tests

- **Unit tests.** `collection/image.rs`, `collection/range_table.rs`,
  `collection/scratch.rs` and `files.rs` carry their own unit tests.
- **Integration tests.** Fixtures are built in-process: a minimal MMDB writer
  and protobuf encoders live in `tests/support`. `tests/image.rs` reopens
  every index type from a mapped file and refuses damaged or foreign bytes.
- **Differential checks.** `tests/real_databases.rs` (ignored by default) runs
  over any directory of real files:
  - MMDB indexes against `maxminddb` lookups with the core's decoding, on
    200 000 addresses per file;
  - `GeoIP.dat` against per-code range membership;
  - `GeoSite.dat` against a linear scan of every rule.

  Each index is stored and reopened from a mapped file before the check. All
  25 files tested agree completely, including both city databases.

## Benchmarks

`examples/bench.rs` reports the process memory growth over a baseline, stage
by stage:

- macOS: `phys_footprint` / RSS;
- Windows: private bytes / shared commit / working set / private working set.
  memmap2's anonymous maps (`read_source`, build buffers) are pagefile-backed
  sections, which Windows counts as shared commit and shared working set, not
  as private bytes;
- Linux: `RssAnon` / `VmRSS`.

It also reports the live heap (from a counting allocator), the build time,
single-thread latency and multi-thread throughput. Each file runs in its own
process (`bench dir <dir>` spawns them), followed by 1 M random lookups (200 k
hosts for GeoSite). `--heap-read` reads the file with `std::fs::read` for
comparison. `--reopen` then stores the index in a temporary file and runs the
same benchmark in a fresh process that maps it, as a cache hit would.

Setup: release profile as shipped (`opt-level = 's'`, LTO).

**macOS**, Apple M3 (4P + 4E), 2026-10-02, measured before indexes moved into
images (the index heap is now the image, in anonymous memory):

| File                                  | Size     | Build  | Index heap | Loaded¹   | Lookup | Raw MMDB² | 8 threads |
| ------------------------------------- | -------- | ------ | ---------- | --------- | ------ | --------- | --------- |
| `Country.mmdb` (bundled, GeoLite2)    | 7.9 MiB  | 70 ms  | 4.71 MiB   | 4.91 MiB  | 30 ns  | 112 ns    | 135 M/s   |
| `geoip.metadb` (Meta-geoip0, default) | 8.0 MiB  | 75 ms  | 5.21 MiB   | 5.59 MiB  | 28 ns  | 60 ns     | 138 M/s   |
| `geoip.db` (sing-geoip)               | 7.8 MiB  | 72 ms  | 5.12 MiB   | 5.31 MiB  | 28 ns  | 58 ns     | 126 M/s   |
| `dbip-country-lite.mmdb`              | 8.0 MiB  | 84 ms  | 5.42 MiB   | 5.62 MiB  | 29 ns  | 156 ns    | 128 M/s   |
| `GeoLite2-City.mmdb` (stress)         | 61.0 MiB | 381 ms | 5.18 MiB   | 5.41 MiB  | 33 ns  | 151 ns    | 123 M/s   |
| `dbip-city-lite.mmdb` (largest)       | 121 MiB  | 808 ms | 5.28 MiB   | 5.52 MiB  | 30 ns  | 228 ns    | 129 M/s   |
| `GeoLite2-ASN.mmdb` (default)         | 11.5 MiB | 98 ms  | 8.36 MiB   | 8.47 MiB  | 41 ns  | 69 ns     | 95 M/s    |
| `dbip-asn-lite.mmdb`                  | 9.1 MiB  | 69 ms  | 7.72 MiB   | 7.84 MiB  | 40 ns  | 87 ns     | 63 M/s    |
| `geoip.dat` (bundled, largest)        | 17.6 MiB | 218 ms | 5.24 MiB   | 5.58 MiB  | 29 ns  | –         | 118 M/s   |
| `geoip.dat` (Loyalsoldier)            | 15.8 MiB | 201 ms | 5.21 MiB   | 5.56 MiB  | 30 ns  | –         | 117 M/s   |
| `geosite.dat` (MetaCubeX default)     | 4.1 MiB  | 115 ms | 3.56 MiB   | 10.63 MiB | 361 ns | –         | 10.0 M/s³ |
| `geosite.dat` (Loyalsoldier, largest) | 10.5 MiB | 244 ms | 7.95 MiB   | 16.98 MiB | 370 ns | –         | 10.3 M/s³ |

**Windows**, Intel Core i9-14900KF (8P + 16E, 32 threads), Windows 11
26200, 2026-10-02:

| File                     | Size     | Build  | Image    | Built⁴: private / shared commit | Reopened⁵: open / private | Lookup | Raw MMDB² | 8 threads | 32 threads |
| ------------------------ | -------- | ------ | -------- | ------------------------------- | ------------------------- | ------ | --------- | --------- | ---------- |
| `Country.mmdb` (bundled) | 7.5 MiB  | 75 ms  | 4.71 MiB | 0.07 / 4.71 MiB                 | 3.0 ms / 0.01 MiB         | 41 ns  | 96 ns     | 176 M/s   | 336 M/s    |
| `geoip.dat` (bundled)    | 16.3 MiB | 239 ms | 5.22 MiB | 0.05 / 5.23 MiB                 | 3.3 ms / 0.01 MiB         | 42 ns  | –         | 171 M/s   | 353 M/s    |
| `geosite.dat` (bundled)  | 4.0 MiB  | 136 ms | 2.71 MiB | 3.47 / 2.71 MiB                 | 9.3 ms / 2.27 MiB         | 381 ns | –         | 3.5 M/s   | 2.8 M/s    |

¹ The growth in `phys_footprint` (macOS) or private bytes (Windows) once the
file is dropped, i.e. what loading costs the process. With `--heap-read`, the
file stays in it on macOS: 12.8 MiB for the bundled `Country.mmdb` and
126.7 MiB for `dbip-city-lite`. The Windows heap returns a block that large,
so `--heap-read` loads the same as `read_source` there.
² Lookup and decode against the whole file kept in memory, which is the
resident alternative to this crate.
³ Measured with `RegexSet`, before lookups shared one regex cache. Its pool
kept a cache per concurrent caller, about 0.8 MiB each: 6.5 MiB after 8
threads, and 25 MiB after 32 threads on Windows. The shared cache adds 0.4 MiB
to the index heap and grows to 0.8 MiB with use, whatever the thread count.
⁴ Once the file is dropped. Windows counts the image, in anonymous memory, as
shared commit. GeoSite's private bytes are the regex set and its cache
(1.26 MiB of live heap) plus freed build heap the allocator keeps.
⁵ In a fresh process that maps the stored image. The working set grows by the
pages lookups touch, all of them file-backed; GeoSite's private bytes are the
regexes compiled again.

IP lookups do not allocate and scale with cores. GeoSite lookups allocate
their result and take the regex cache lock, so their throughput peaks around
4 threads (about 5 M/s on Windows) and falls under contention; single-thread
latency is unchanged. Multi-thread throughput varies by up to a third between
runs. Linux figures are not measured yet.

IP lookups through an image take about 40 ns on Windows against 35 ns with
owned arrays: each lookup reaches its sections through the image's owner.
