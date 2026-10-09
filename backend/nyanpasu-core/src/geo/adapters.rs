//! The core's country database on disk, and the index images cached beside it.
use std::{
    fs::{self, File},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use notify_debouncer_full::{DebounceEventResult, new_debouncer, notify::RecursiveMode};
use nyanpasu_geodata::{IpIndex, MihomoGeoFiles, read_source};
use sha2::{Digest, Sha256};
use snafu::ResultExt as _;

use super::ports::{
    CountryIndexSource, GeoIndexError, GeodataMode, IndexDatabaseSnafu, IndexKey, ListHomeSnafu,
    Loaded, OnChange, ReadDatabaseSnafu, WatchHomeSnafu,
};

/// The names the core resolves its country database from, compared ignoring ASCII case.
const COUNTRY_DATABASES: [&str; 4] = ["Country.mmdb", "geoip.db", "geoip.metadb", "GeoIP.dat"];

/// The core rewrites a database in one write; this outlasts it.
const WATCH_DEBOUNCE: Duration = Duration::from_secs(2);

pub struct FsCountryIndexSource {
    /// The core's home: `-d` of every core the app starts.
    home: PathBuf,
    /// Index images, named by the content they were built from.
    cache: PathBuf,
}

impl FsCountryIndexSource {
    pub fn new(home: PathBuf, cache: PathBuf) -> Self {
        Self { home, cache }
    }

    /// The index stored in `image`; `None` when there is none or it was rejected and removed.
    fn reopen(&self, image: &Path) -> io::Result<Option<IpIndex>> {
        let file = match open_image(image) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        // SAFETY: an image is never written after it is renamed into place: new content gets a
        // new name, and a stale image is only ever removed.
        let map = unsafe { memmap2::Mmap::map(&file)? };
        match IpIndex::from_bytes(map) {
            Ok(index) => Ok(Some(index)),
            Err(error) => {
                tracing::warn!(image = %image.display(), %error, "rebuilding a rejected index image");
                fs::remove_file(image)?;
                Ok(None)
            }
        }
    }

    fn store(&self, image: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.cache)?;
        let mut staged = tempfile::NamedTempFile::new_in(&self.cache)?;
        staged.write_all(bytes)?;
        staged.persist(image).map_err(|error| error.error)?;
        Ok(())
    }

    /// Best effort: an image another index still maps may refuse removal until the next switch.
    fn remove_other_images(&self, key: &IndexKey) {
        let keep = image_name(key);
        let prefix = image_prefix(key.mode);
        let Ok(entries) = fs::read_dir(&self.cache) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with(&prefix)
                && name != keep
                && let Err(error) = fs::remove_file(entry.path())
            {
                tracing::debug!(image = name, %error, "a stale index image stays for now");
            }
        }
    }
}

impl CountryIndexSource for FsCountryIndexSource {
    fn load(&self, mode: GeodataMode, current: Option<IndexKey>) -> Result<Loaded, GeoIndexError> {
        let files = MihomoGeoFiles::discover(&self.home).context(ListHomeSnafu)?;
        let path = match mode {
            GeodataMode::Mmdb => files.ip_mmdb,
            GeodataMode::Dat => files.geoip_dat,
        };
        let Some(path) = path else {
            return Ok(Loaded::Missing);
        };
        let source = read_source(&path).context(ReadDatabaseSnafu { path: path.clone() })?;
        let key = IndexKey {
            mode,
            sha256: Sha256::digest(&*source).into(),
        };
        if current.as_ref() == Some(&key) {
            return Ok(Loaded::Unchanged);
        }
        let image = self.cache.join(image_name(&key));
        let cached = self.reopen(&image).unwrap_or_else(|error| {
            tracing::warn!(image = %image.display(), %error, "the index image is unreadable");
            None
        });
        let index = match cached {
            Some(index) => index,
            None => {
                let built = match mode {
                    GeodataMode::Mmdb => IpIndex::from_mmdb(&source),
                    GeodataMode::Dat => IpIndex::from_geoip_dat(&source),
                }
                .context(IndexDatabaseSnafu { path })?;
                drop(source);
                // The image is only a cache: without it the index still serves, from
                // anonymous memory instead of reclaimable file pages.
                match self
                    .store(&image, built.as_bytes())
                    .and_then(|()| self.reopen(&image))
                {
                    Ok(Some(reopened)) => reopened,
                    Ok(None) => built,
                    Err(error) => {
                        tracing::warn!(image = %image.display(), %error, "the index was not cached");
                        built
                    }
                }
            }
        };
        self.remove_other_images(&key);
        Ok(Loaded::Index {
            key,
            index: Arc::new(index),
        })
    }

    fn watch(&self, changed: OnChange) -> Result<Box<dyn Send>, GeoIndexError> {
        let mut debouncer = new_debouncer(
            WATCH_DEBOUNCE,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    if events
                        .iter()
                        .flat_map(|event| &event.paths)
                        .any(|path| is_country_database(path))
                    {
                        changed.notify();
                    }
                }
                Err(errors) => tracing::warn!(?errors, "the core's home could not be watched"),
            },
        )
        .context(WatchHomeSnafu)?;
        debouncer
            .watch(&self.home, RecursiveMode::NonRecursive)
            .context(WatchHomeSnafu)?;
        Ok(Box::new(debouncer))
    }
}

fn is_country_database(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            COUNTRY_DATABASES
                .iter()
                .any(|database| name.eq_ignore_ascii_case(database))
        })
}

fn image_prefix(mode: GeodataMode) -> String {
    let mode = match mode {
        GeodataMode::Mmdb => "mmdb",
        GeodataMode::Dat => "dat",
    };
    format!("country-{mode}-")
}

fn image_name(key: &IndexKey) -> String {
    format!("{}{}.idx", image_prefix(key.mode), hex::encode(key.sha256))
}

fn open_image(image: &Path) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // FILE_SHARE_READ | FILE_SHARE_DELETE: other readers and a later cleanup may proceed,
        // a writer may not while the image is mapped.
        options.share_mode(0x1 | 0x4);
    }
    options.open(image)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::geo::fixtures::geoip_dat;

    struct Home {
        dir: tempfile::TempDir,
    }

    impl Home {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn source(&self) -> FsCountryIndexSource {
            FsCountryIndexSource::new(self.dir.path().into(), self.dir.path().join("cache"))
        }

        fn write(&self, name: &str, bytes: &[u8]) {
            fs::write(self.dir.path().join(name), bytes).unwrap();
        }

        fn images(&self) -> Vec<String> {
            let Ok(entries) = fs::read_dir(self.dir.path().join("cache")) else {
                return Vec::new();
            };
            let mut names: Vec<String> = entries
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }

    fn index(loaded: Loaded) -> (IndexKey, Arc<IpIndex>) {
        match loaded {
            Loaded::Index { key, index } => (key, index),
            Loaded::Unchanged => panic!("expected an index, got Unchanged"),
            Loaded::Missing => panic!("expected an index, got Missing"),
        }
    }

    fn country(index: &IpIndex, ip: &str) -> Option<String> {
        let ip: IpAddr = ip.parse().unwrap();
        index.lookup(ip)?.country().map(str::to_owned)
    }

    #[test]
    fn a_home_without_the_mode_s_database_has_none() {
        let home = Home::new();
        home.write("GeoIP.dat", &geoip_dat(&[("US", &["8.0.0.0/8"])]));

        assert!(matches!(
            home.source().load(GeodataMode::Mmdb, None).unwrap(),
            Loaded::Missing
        ));
        let (_, index) = index(home.source().load(GeodataMode::Dat, None).unwrap());
        assert_eq!(country(&index, "8.8.8.8").as_deref(), Some("us"));
    }

    #[test]
    fn the_same_content_is_unchanged() {
        let home = Home::new();
        home.write("geoip.dat", &geoip_dat(&[("US", &["8.0.0.0/8"])]));
        let source = home.source();
        let (key, _) = index(source.load(GeodataMode::Dat, None).unwrap());

        assert!(matches!(
            source.load(GeodataMode::Dat, Some(key.clone())).unwrap(),
            Loaded::Unchanged
        ));
    }

    #[test]
    fn a_later_load_reopens_the_cached_image() {
        let home = Home::new();
        home.write("GeoIP.dat", &geoip_dat(&[("JP", &["1.0.16.0/20"])]));
        let (first, index_a) = index(home.source().load(GeodataMode::Dat, None).unwrap());
        let images = home.images();
        let image = home.dir.path().join("cache").join(&images[0]);
        let written = fs::metadata(&image).unwrap().modified().unwrap();

        let (second, index_b) = index(home.source().load(GeodataMode::Dat, None).unwrap());

        assert_eq!(first, second);
        assert_eq!(images.len(), 1);
        assert_eq!(home.images(), images);
        assert_eq!(fs::metadata(&image).unwrap().modified().unwrap(), written);
        assert_eq!(country(&index_a, "1.0.16.1").as_deref(), Some("jp"));
        assert_eq!(country(&index_b, "1.0.16.1").as_deref(), Some("jp"));
    }

    #[test]
    fn a_damaged_image_is_rebuilt() {
        let home = Home::new();
        home.write("GeoIP.dat", &geoip_dat(&[("DE", &["5.0.0.0/8"])]));
        drop(home.source().load(GeodataMode::Dat, None).unwrap());
        let image = home.dir.path().join("cache").join(&home.images()[0]);
        fs::write(&image, b"not an index").unwrap();

        let (_, index) = index(home.source().load(GeodataMode::Dat, None).unwrap());

        assert_eq!(country(&index, "5.1.2.3").as_deref(), Some("de"));
        assert_ne!(fs::read(&image).unwrap(), b"not an index");
    }

    #[test]
    fn new_content_replaces_the_previous_image() {
        let home = Home::new();
        home.write("GeoIP.dat", &geoip_dat(&[("FR", &["2.0.0.0/8"])]));
        let (first, _) = index(home.source().load(GeodataMode::Dat, None).unwrap());
        home.write("GeoIP.dat", &geoip_dat(&[("IT", &["2.0.0.0/8"])]));

        let (second, index) = index(
            home.source()
                .load(GeodataMode::Dat, Some(first.clone()))
                .unwrap(),
        );

        assert_ne!(first, second);
        assert_eq!(country(&index, "2.3.4.5").as_deref(), Some("it"));
        assert_eq!(home.images(), [image_name(&second)]);
    }

    #[test]
    fn only_country_databases_count_as_changes() {
        assert!(is_country_database(Path::new("/core/country.MMDB")));
        assert!(is_country_database(Path::new("/core/GEOIP.dat")));
        assert!(is_country_database(Path::new("/core/geoip.metadb")));
        assert!(!is_country_database(Path::new("/core/GeoSite.dat")));
        assert!(!is_country_database(Path::new("/core/cache.db")));
    }

    #[test]
    fn writing_the_database_is_reported() {
        let home = Home::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        let guard = home
            .source()
            .watch(OnChange::new(move || {
                let _ = sender.send(());
            }))
            .unwrap();

        home.write("GeoIP.dat", &geoip_dat(&[("US", &["8.0.0.0/8"])]));

        receiver.recv_timeout(Duration::from_secs(20)).unwrap();
        drop(guard);
    }
}
