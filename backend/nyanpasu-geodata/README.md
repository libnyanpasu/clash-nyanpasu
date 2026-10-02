# nyanpasu-geodata

Read-only lookups over the geo databases the mihomo core already keeps in its
home directory, so the app can locate connections the core left without
`destinationGeoIP` / `destinationIPASN` (the core fills those only while a
`GEOIP` / `IP-ASN` rule runs).

Each file is compiled once into a compact, immutable index and the source bytes
are released. Nothing holds the core's files open or mapped, so the core can
rewrite them in place (`geo-auto-update`) while an index is alive. Indexes are
`Send + Sync`; share them behind an `Arc` and rebuild to reload.

| File (as the core resolves it)               | Index                         |
| -------------------------------------------- | ----------------------------- |
| `Country.mmdb` / `geoip.db` / `geoip.metadb` | `IpIndex::from_mmdb`          |
| `ASN.mmdb`                                   | `AsnIndex::from_mmdb`         |
| `GeoIP.dat` (`geodata-mode: true`)           | `IpIndex::from_geoip_dat`     |
| `GeoSite.dat`                                | `SiteIndex::from_geosite_dat` |

```rust
use nyanpasu_geodata::{IpIndex, MihomoGeoFiles, read_source};

let files = MihomoGeoFiles::discover(core_home)?;
let index = IpIndex::from_mmdb(&read_source(&files.ip_mmdb.unwrap())?)?;
if let Some(tags) = index.lookup("8.8.8.8".parse()?) {
    // ["us"], or e.g. ["google", "us"] from a Meta-geoip0 database
    let country = tags.country();
}
```

Read files with `read_source` rather than `std::fs::read`. The system
allocator keeps freed heap pages, so a file read onto the heap would stay in
the process footprint after the build; `read_source` uses anonymous memory,
which the OS takes back when the source drops.

An index lives in one block of bytes. Store `index.as_bytes()` in a file and
reopen it later with `IpIndex::from_bytes` (or `AsnIndex` / `SiteIndex`) over a
read-only memory map of that file: opening takes milliseconds instead of a
rebuild, and the mapped pages are file-backed, so they stay out of the
process's private memory. Key stored indexes by the source content, replace
them by renaming a new file into place, and rebuild whenever `from_bytes`
fails.

Building is synchronous CPU work (tens to hundreds of milliseconds); async
callers run it on a blocking thread. `examples/bench.rs` measures load time,
memory and lookups on any platform:
`cargo run --release -p nyanpasu-geodata --example bench -- dir <core home>`.

See [DESIGN.md](DESIGN.md) for the index layouts, the core-compatibility
notes and benchmark results.
