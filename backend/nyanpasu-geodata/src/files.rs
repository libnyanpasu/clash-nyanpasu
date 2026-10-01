use std::{
    io::{self, Read},
    ops::Deref,
    path::{Path, PathBuf},
};

use crate::scratch::ScratchVec;

/// The geo files the mihomo core loads from its home directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MihomoGeoFiles {
    /// `Country.mmdb`, else `geoip.db`, else `geoip.metadb`.
    pub ip_mmdb: Option<PathBuf>,
    pub asn_mmdb: Option<PathBuf>,
    /// Used instead of `ip_mmdb` under `geodata-mode: true`.
    pub geoip_dat: Option<PathBuf>,
    pub geosite_dat: Option<PathBuf>,
}

impl MihomoGeoFiles {
    /// Resolves names as the core does: the first entry in byte order whose
    /// name equals a candidate ignoring ASCII case; directories don't count.
    pub fn discover(home: &Path) -> io::Result<Self> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(home)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                names.push(entry.file_name());
            }
        }
        names.sort();
        let find = |candidates: &[&str]| {
            names
                .iter()
                .find(|name| {
                    name.to_str().is_some_and(|name| {
                        candidates
                            .iter()
                            .any(|candidate| name.eq_ignore_ascii_case(candidate))
                    })
                })
                .map(|name| home.join(name))
        };
        Ok(Self {
            ip_mmdb: find(&["Country.mmdb", "geoip.db", "geoip.metadb"]),
            asn_mmdb: find(&["ASN.mmdb"]),
            geoip_dat: find(&["GeoIP.dat"]),
            geosite_dat: find(&["GeoSite.dat"]),
        })
    }
}

/// A database file's bytes, read for one build.
pub struct Source(ScratchVec<u8>);

impl Deref for Source {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Reads `path` into anonymous memory that goes back to the OS when the
/// `Source` drops; the system allocator would keep heap pages the size of the
/// file. It is a copy, so the core may rewrite the file while an index is
/// built from it.
pub fn read_source(path: &Path) -> io::Result<Source> {
    let mut file = std::fs::File::open(path)?;
    let len = usize::try_from(file.metadata()?.len()).map_err(io::Error::other)?;
    let mut bytes = ScratchVec::zeroed(len).map_err(io::Error::other)?;
    file.read_exact(bytes.as_mut_slice())?;
    // A file still growing is being rewritten, and a prefix of a `.dat` list
    // can parse as a shorter list.
    if file.read(&mut [0])? != 0 {
        return Err(io::Error::other("file grew while being read"));
    }
    Ok(Source(bytes))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn home(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            fs::write(dir.path().join(file), b"").unwrap();
        }
        dir
    }

    fn name(path: Option<PathBuf>) -> Option<String> {
        path.map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
    }

    #[test]
    fn a_source_holds_the_file_bytes() {
        let dir = home(&[]);
        let path = dir.path().join("GeoIP.dat");
        let bytes: Vec<u8> = (0..10_000u32).map(|i| (i * 7) as u8).collect();
        fs::write(&path, &bytes).unwrap();

        assert_eq!(&*read_source(&path).unwrap(), bytes.as_slice());
    }

    #[test]
    fn an_empty_file_is_an_empty_source() {
        let dir = home(&["GeoIP.dat"]);

        assert!(
            read_source(&dir.path().join("GeoIP.dat"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let dir = home(&[]);

        assert!(read_source(&dir.path().join("GeoIP.dat")).is_err());
    }

    #[test]
    fn country_mmdb_wins_over_the_other_ip_databases() {
        let dir = home(&["geoip.metadb", "geoip.db", "Country.mmdb"]);
        let files = MihomoGeoFiles::discover(dir.path()).unwrap();

        assert_eq!(name(files.ip_mmdb), Some("Country.mmdb".to_owned()));
    }

    #[test]
    fn geoip_db_wins_over_geoip_metadb() {
        let dir = home(&["geoip.metadb", "geoip.db"]);
        let files = MihomoGeoFiles::discover(dir.path()).unwrap();

        assert_eq!(name(files.ip_mmdb), Some("geoip.db".to_owned()));
    }

    #[test]
    fn names_match_ignoring_ascii_case() {
        let dir = home(&["country.MMDB", "asn.mmdb", "GEOIP.dat", "geosite.DAT"]);
        let files = MihomoGeoFiles::discover(dir.path()).unwrap();

        assert_eq!(name(files.ip_mmdb), Some("country.MMDB".to_owned()));
        assert_eq!(name(files.asn_mmdb), Some("asn.mmdb".to_owned()));
        assert_eq!(name(files.geoip_dat), Some("GEOIP.dat".to_owned()));
        assert_eq!(name(files.geosite_dat), Some("geosite.DAT".to_owned()));
    }

    #[test]
    fn directories_and_absent_files_resolve_to_nothing() {
        let dir = home(&[]);
        fs::create_dir(dir.path().join("Country.mmdb")).unwrap();
        let files = MihomoGeoFiles::discover(dir.path()).unwrap();

        assert_eq!(files, MihomoGeoFiles::default());
    }
}
