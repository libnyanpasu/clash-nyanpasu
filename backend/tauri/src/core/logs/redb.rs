use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, bail, ensure};
use redb::{
    BackendError, Database, ReadTransaction, ReadableDatabase, ReadableTable, StorageBackend,
    TableDefinition, backends::FileBackend,
};
use serde::{Deserialize, Serialize};

use super::{model::*, ports::CoreLogStore};

const LOGS: TableDefinition<u64, &[u8]> = TableDefinition::new("logs");
const LEVELS: TableDefinition<(u8, u64), u8> = TableDefinition::new("levels");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const CACHE_BYTES: usize = 1024 * 1024;
const READ_CACHE_BYTES: usize = 256 * 1024;
const CONTROL_RESERVE: u64 = 16 * 1024;
const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub total: u64,
    pub segment: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            total: 64 * 1024 * 1024,
            segment: 20 * 1024 * 1024,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Control {
    version: u32,
    generation: String,
    next_segment: u64,
}
#[derive(Default, Serialize, Deserialize)]
struct Header {
    version: u32,
    first: u64,
    last: u64,
}
struct Segment {
    number: u64,
    path: PathBuf,
    header: Header,
}

/// A single owner keeps the directory lock, sole writer and small shard catalog.
/// Old shards are opened read-only for one query and closed before any rotation.
pub struct RedbCoreLogStore {
    directory: PathBuf,
    _lock: File,
    control: Control,
    limits: Limits,
    segments: Vec<Segment>,
    active: Option<Database>,
    quota_hit: Arc<AtomicBool>,
    clearing: bool,
    // Kept until recovery establishes the commit outcome; the actor retries this same batch.
    uncertain: Option<(u64, usize)>,
}

#[derive(Debug)]
struct CappedFileBackend {
    inner: FileBackend,
    limit: u64,
    quota_hit: Arc<AtomicBool>,
}
impl CappedFileBackend {
    fn check(&self, end: u64) -> io::Result<()> {
        if end > self.limit {
            self.quota_hit.store(true, Ordering::Release);
            return Err(io::Error::other("Core log segment size limit reached"));
        }
        Ok(())
    }
}
impl StorageBackend for CappedFileBackend {
    fn len(&self) -> io::Result<u64> {
        self.inner.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        self.inner.read(offset, out)
    }
    fn set_len(&self, len: u64) -> io::Result<()> {
        self.check(len)?;
        self.inner.set_len(len)
    }
    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.check(
            offset
                .checked_add(data.len() as u64)
                .ok_or_else(|| io::Error::other("log write offset overflow"))?,
        )?;
        self.inner.write(offset, data)
    }
    fn sync_data(&self) -> io::Result<()> {
        self.inner.sync_data()
    }
    fn close(&self) -> io::Result<()> {
        self.inner.close()
    }
    fn try_lock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<bool, BackendError> {
        self.inner.try_lock_range(start, end)
    }
    fn try_lock_shared_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> Result<bool, BackendError> {
        self.inner.try_lock_shared_range(start, end)
    }
    fn lock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<(), BackendError> {
        self.inner.lock_range(start, end)
    }
    fn lock_shared_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<(), BackendError> {
        self.inner.lock_shared_range(start, end)
    }
    fn unlock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<(), BackendError> {
        self.inner.unlock_range(start, end)
    }
    fn query_lock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<bool, BackendError> {
        self.inner.query_lock_range(start, end)
    }
}

fn map_error(error: anyhow::Error) -> CoreLogError {
    match error.downcast::<CoreLogError>() {
        Ok(error) => error,
        Err(error) => error.into(),
    }
}

fn directory_bytes(directory: &Path) -> Result<u64> {
    let mut total = 0_u64;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "unexpected entry in Core log directory"
        );
        total = total
            .checked_add(entry.metadata()?.len())
            .context("Core log directory size overflow")?;
    }
    Ok(total)
}

fn read_header(transaction: &ReadTransaction) -> Result<Header> {
    let table = transaction.open_table(META)?;
    let value = table
        .get("header")?
        .context("missing Core log shard header")?;
    let header: Header = serde_json::from_slice(value.value())?;
    ensure!(
        header.version == FORMAT_VERSION,
        "unsupported Core log shard version"
    );
    Ok(header)
}

impl RedbCoreLogStore {
    pub fn open(directory: PathBuf) -> Result<Self> {
        Self::open_with_limits(directory, Limits::default())
    }

    pub(super) fn open_with_limits(directory: PathBuf, limits: Limits) -> Result<Self> {
        ensure!(
            limits.segment * 2 + CONTROL_RESERVE <= limits.total,
            "invalid Core log budget"
        );
        fs::create_dir_all(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("owner.lock"))?;
        lock.try_lock()
            .context("Core log directory already owned")?;
        let control_path = directory.join("control.json");
        let control = if control_path.exists() {
            let bytes = fs::read(&control_path)?;
            ensure!(bytes.len() <= 4096, "invalid Core log control file");
            let control: Control = serde_json::from_slice(&bytes)?;
            ensure!(
                control.version == FORMAT_VERSION,
                "unsupported Core log store version"
            );
            uuid::Uuid::parse_str(&control.generation)?;
            control
        } else {
            ensure!(
                fs::read_dir(&directory)?
                    .all(|entry| entry.is_ok_and(|entry| entry.file_name() == "owner.lock")),
                "missing Core log control file in nonempty directory"
            );
            Control {
                version: FORMAT_VERSION,
                generation: uuid::Uuid::new_v4().to_string(),
                next_segment: 1,
            }
        };
        let mut store = Self {
            directory,
            _lock: lock,
            control,
            limits,
            segments: Vec::new(),
            active: None,
            quota_hit: Arc::new(AtomicBool::new(false)),
            clearing: false,
            uncertain: None,
        };
        store.write_control()?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(&store.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(stem) = name.strip_suffix(".redb") else {
                continue;
            };
            let Some((generation, number)) = stem.rsplit_once('.') else {
                continue;
            };
            if uuid::Uuid::parse_str(generation).is_err() {
                continue;
            }
            let Ok(number) = number.parse::<u64>() else {
                continue;
            };
            ensure!(entry.file_type()?.is_file(), "invalid Core log shard path");
            // A durable generation change is the clear intent. Finish it after a crash.
            if generation != store.control.generation {
                fs::remove_file(entry.path())?;
                continue;
            }
            paths.push((number, entry.path()));
        }
        paths.sort_by_key(|(number, _)| *number);
        for (number, path) in &paths {
            ensure!(
                *number < store.control.next_segment,
                "invalid Core log shard order"
            );
            ensure!(
                fs::metadata(path)?.len() <= limits.segment,
                "Core log shard exceeds its budget"
            );
            store.segments.push(Segment {
                number: *number,
                path: path.clone(),
                header: Header::default(),
            });
        }
        if !store.segments.is_empty() {
            store.reserve_active(true)?;
            let path = store.segments.last().unwrap().path.clone();
            store.active = Some(store.open_writer(&path, false)?);
            for index in 0..store.segments.len() {
                let header = store.with_read(index, read_header)?;
                store.segments[index].header = header;
            }
        } else {
            store.create_segment()?;
        }
        Ok(store)
    }

    fn write_control(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.control)?;
        ensure!(
            bytes.len() <= 4096,
            "Core log control metadata exceeds reserve"
        );
        ensure!(
            directory_bytes(&self.directory)? + bytes.len() as u64 <= self.limits.total,
            "Core log directory budget exhausted"
        );
        let temporary = self.directory.join("control.tmp");
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, self.directory.join("control.json"))?;
        #[cfg(unix)]
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    fn open_writer(&self, path: &Path, create: bool) -> Result<Database> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(create)
            .open(path)?;
        let backend = CappedFileBackend {
            inner: FileBackend::new(file)?,
            limit: self.limits.segment,
            quota_hit: self.quota_hit.clone(),
        };
        let mut builder = Database::builder();
        builder.set_cache_size(CACHE_BYTES);
        let database = builder.create_with_backend(backend)?;
        // A crash during empty-shard creation can leave an initialized DB without its tables.
        let transaction = database.begin_write()?;
        transaction.open_table(LOGS)?;
        transaction.open_table(LEVELS)?;
        {
            let mut meta = transaction.open_table(META)?;
            if meta.get("header")?.is_none() {
                let header = serde_json::to_vec(&Header {
                    version: FORMAT_VERSION,
                    ..Default::default()
                })?;
                meta.insert("header", header.as_slice())?;
            }
        }
        transaction.commit()?;
        Ok(database)
    }

    fn reserve_active(&mut self, keep_latest: bool) -> Result<()> {
        let active_bytes = if keep_latest {
            self.segments
                .last()
                .map(|s| fs::metadata(&s.path).map(|m| m.len()))
                .transpose()?
                .unwrap_or(0)
        } else {
            0
        };
        // Reserve the entire maximum growth before opening/creating the writer.
        while directory_bytes(&self.directory)?.saturating_sub(active_bytes)
            + self.limits.segment
            + CONTROL_RESERVE
            > self.limits.total
        {
            ensure!(
                !self.segments.is_empty() && (!keep_latest || self.segments.len() > 1),
                "Core log directory budget exhausted"
            );
            fs::remove_file(&self.segments[0].path)
                .context("failed to evict oldest Core log shard")?;
            self.segments.remove(0);
        }
        Ok(())
    }

    fn create_segment(&mut self) -> Result<()> {
        self.reserve_active(false)?;
        let number = self.control.next_segment;
        self.control.next_segment = number
            .checked_add(1)
            .context("Core log shard sequence exhausted")?;
        self.write_control()?;
        let path = self
            .directory
            .join(format!("{}.{number:020}.redb", self.control.generation));
        // Create the path separately so cleanup can never remove a pre-existing file.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let database = match self.open_writer(&path, false) {
            Ok(database) => database,
            Err(error) => {
                fs::remove_file(&path).context("failed to remove incomplete Core log shard")?;
                return Err(error);
            }
        };
        let header = read_header(&database.begin_read()?)?;
        self.segments.push(Segment {
            number,
            path,
            header,
        });
        self.active = Some(database);
        Ok(())
    }

    fn with_read<T>(
        &self,
        index: usize,
        operation: impl FnOnce(&ReadTransaction) -> Result<T>,
    ) -> Result<T> {
        if index + 1 == self.segments.len()
            && let Some(database) = &self.active
        {
            return operation(&database.begin_read()?);
        }
        let mut builder = Database::builder();
        builder.set_cache_size(READ_CACHE_BYTES);
        let database = builder.open_read_only(&self.segments[index].path)?;
        operation(&database.begin_read()?)
    }

    fn cursor(&self, segment: u64, sequence: u64) -> CoreLogCursor {
        CoreLogCursor {
            generation: self.control.generation.clone(),
            segment,
            sequence,
        }
    }

    fn current_status(&self) -> Result<CoreLogStatus> {
        ensure!(!self.clearing, "Core log clear is incomplete");
        let first = self
            .segments
            .iter()
            .find(|s| s.header.first > 0)
            .map(|s| self.cursor(s.number, s.header.first));
        let head = self
            .segments
            .iter()
            .rev()
            .find(|s| s.header.last > 0)
            .map(|s| self.cursor(s.number, s.header.last));
        Ok(CoreLogStatus {
            generation: self.control.generation.clone(),
            first,
            head,
            bytes: directory_bytes(&self.directory)?,
            budget: self.limits.total,
            ..Default::default()
        })
    }

    fn append_batch(&mut self, records: &[Vec<u8>]) -> Result<()> {
        if self.clearing {
            self.finish_clear()?;
        }
        if records.is_empty() {
            return Ok(());
        }
        if self.uncertain.is_some() && self.recover_outcome(records)? {
            return Ok(());
        }
        for bytes in records {
            if bytes.len() > MAX_RECORD_BYTES {
                bail!(CoreLogError::TooLarge);
            }
            let record: CoreLogRecord = serde_json::from_slice(bytes)?;
            if !record.metadata_fits() {
                bail!(CoreLogError::TooLarge);
            }
        }
        if self.active.is_none() {
            self.create_segment()?;
        }
        let previous = self.segments.last().unwrap().header.last;
        let write = |database: &Database, first: u64| -> Result<()> {
            let transaction = database.begin_write()?;
            {
                let mut logs = transaction.open_table(LOGS)?;
                let mut levels = transaction.open_table(LEVELS)?;
                let mut meta = transaction.open_table(META)?;
                let mut header: Header = serde_json::from_slice(
                    meta.get("header")?
                        .context("missing Core log header")?
                        .value(),
                )?;
                for (index, bytes) in records.iter().enumerate() {
                    let sequence = first
                        .checked_add(index as u64)
                        .context("Core log record sequence exhausted")?;
                    let record: CoreLogRecord = serde_json::from_slice(bytes)?;
                    logs.insert(sequence, bytes.as_slice())?;
                    levels.insert((level_code(&record.log_type), sequence), 0)?;
                    if header.first == 0 {
                        header.first = sequence;
                    }
                    header.last = sequence;
                }
                let encoded = serde_json::to_vec(&header)?;
                meta.insert("header", encoded.as_slice())?;
            }
            transaction.commit()?;
            Ok(())
        };
        self.quota_hit.store(false, Ordering::Release);
        self.uncertain = Some((previous, records.len()));
        if let Err(error) = write(
            self.active.as_ref().unwrap(),
            previous
                .checked_add(1)
                .context("Core log record sequence exhausted")?,
        ) {
            let quota = self.quota_hit.swap(false, Ordering::AcqRel);
            // Recovery determines the real commit outcome before any replay.
            drop(self.active.take());
            if self.recover_outcome(records)? {
                return Ok(());
            }
            if !quota {
                return Err(error);
            }
            if previous == 0 {
                bail!(CoreLogError::TooLarge);
            }
            drop(self.active.take());
            self.create_segment()?;
            self.uncertain = Some((0, records.len()));
            if let Err(error) = write(self.active.as_ref().unwrap(), 1) {
                drop(self.active.take());
                if self.recover_outcome(records)? {
                    return Ok(());
                }
                return Err(error);
            }
        }
        ensure!(
            self.recover_outcome(records)?,
            "Core log commit did not advance its header"
        );
        Ok(())
    }

    fn recover_outcome(&mut self, records: &[Vec<u8>]) -> Result<bool> {
        let (previous, count) = self.uncertain.context("missing Core log batch outcome")?;
        ensure!(
            count == records.len(),
            "Core log retry must retain its pending batch"
        );
        if self.active.is_none() {
            let path = self
                .segments
                .last()
                .context("missing Core log recovery shard")?
                .path
                .clone();
            self.active = Some(self.open_writer(&path, false)?);
        }
        let transaction = self.active.as_ref().unwrap().begin_read()?;
        let header = read_header(&transaction)?;
        let expected = previous
            .checked_add(count as u64)
            .context("Core log record sequence exhausted")?;
        let committed = header.last == expected;
        ensure!(
            committed || header.last == previous,
            "ambiguous Core log batch outcome"
        );
        if committed {
            let table = transaction.open_table(LOGS)?;
            for (index, bytes) in records.iter().enumerate() {
                let stored = table
                    .get(previous + index as u64 + 1)?
                    .context("missing committed Core log record")?;
                ensure!(
                    stored.value() == bytes.as_slice(),
                    "Core log retry differs from committed batch"
                );
            }
        }
        self.segments.last_mut().unwrap().header = header;
        self.uncertain = None;
        Ok(committed)
    }

    fn validate_cursor(&self, cursor: &CoreLogCursor) -> Result<()> {
        ensure!(!self.clearing, "Core log clear is incomplete");
        if cursor.generation != self.control.generation
            || !self.segments.iter().any(|s| {
                s.number == cursor.segment
                    && cursor.sequence >= s.header.first
                    && cursor.sequence <= s.header.last
            })
        {
            bail!(CoreLogError::CursorExpired);
        }
        Ok(())
    }

    fn query_page(&self, query: CoreLogQuery) -> Result<CoreLogPage> {
        if query.limit == 0
            || query.limit > MAX_PAGE_ROWS
            || query.keyword.len() > 1024
            || query.level.as_ref().is_some_and(|s| s.len() > 128)
            || (query.direction == CoreLogDirection::Latest) != query.cursor.is_none()
        {
            bail!(CoreLogError::InvalidRequest);
        }
        if let Some(cursor) = &query.cursor {
            self.validate_cursor(cursor)?;
        }
        let status = self.current_status()?;
        let mut page = CoreLogPage {
            rows: Vec::new(),
            cursor: query.cursor.clone(),
            more: false,
            status,
        };
        let backwards = query.direction != CoreLogDirection::After;
        let level = query.level.as_deref().map(normalize_level);
        let keyword = query.keyword.to_lowercase();
        let mut scanned = 0;
        let mut scan_bytes = 0;
        let mut response_bytes = 4096;
        let indices: Box<dyn Iterator<Item = usize>> = if backwards {
            Box::new((0..self.segments.len()).rev())
        } else {
            Box::new(0..self.segments.len())
        };
        for index in indices {
            let segment = &self.segments[index];
            if segment.header.first == 0 {
                continue;
            }
            if let Some(cursor) = &query.cursor
                && (backwards && segment.number > cursor.segment
                    || !backwards && segment.number < cursor.segment)
            {
                continue;
            }
            let stopped = self.with_read(index, |transaction| {
                let logs = transaction.open_table(LOGS)?;
                let mut consume = |sequence: u64, bytes: &[u8]| -> Result<bool> {
                    ensure!(
                        bytes.len() <= MAX_RECORD_BYTES,
                        "Core log record exceeds its size limit"
                    );
                    let cursor = self.cursor(segment.number, sequence);
                    if let Some(bound) = &query.cursor
                        && (backwards && cursor >= *bound || !backwards && cursor <= *bound)
                    {
                        return Ok(false);
                    }
                    if scanned >= MAX_SCAN_ROWS || scan_bytes + bytes.len() > MAX_SCAN_BYTES {
                        page.more = true;
                        return Ok(true);
                    }
                    let mut record: CoreLogRecord = serde_json::from_slice(bytes)?;
                    let matches = level
                        .as_ref()
                        .is_none_or(|level| normalize_level(&record.log_type) == *level)
                        && (keyword.is_empty() || record.payload.to_lowercase().contains(&keyword));
                    if matches {
                        let truncated = truncate_preview(&mut record.payload);
                        let row = CoreLogRow {
                            id: cursor.clone(),
                            record,
                            truncated,
                        };
                        let length = serde_json::to_vec(&row)?.len();
                        if page.rows.len() >= query.limit || response_bytes + length > PAGE_BYTES {
                            page.more = true;
                            return Ok(true);
                        }
                        response_bytes += length;
                        page.rows.push(row);
                    }
                    scanned += 1;
                    scan_bytes += bytes.len();
                    page.cursor = Some(cursor);
                    Ok(false)
                };
                let lower = if query
                    .cursor
                    .as_ref()
                    .is_some_and(|c| c.segment == segment.number)
                    && !backwards
                {
                    Bound::Excluded(query.cursor.as_ref().unwrap().sequence)
                } else {
                    Bound::Unbounded
                };
                let upper = if query
                    .cursor
                    .as_ref()
                    .is_some_and(|c| c.segment == segment.number)
                    && backwards
                {
                    Bound::Excluded(query.cursor.as_ref().unwrap().sequence)
                } else {
                    Bound::Unbounded
                };
                if let Some(level) = &level {
                    let code = level_code(level);
                    let start = match lower {
                        Bound::Excluded(n) => Bound::Excluded((code, n)),
                        _ => Bound::Included((code, 0)),
                    };
                    let end = match upper {
                        Bound::Excluded(n) => Bound::Excluded((code, n)),
                        _ => Bound::Included((code, u64::MAX)),
                    };
                    let levels = transaction.open_table(LEVELS)?;
                    let range = levels.range((start, end))?;
                    let items: Box<dyn Iterator<Item = _>> = if backwards {
                        Box::new(range.rev())
                    } else {
                        Box::new(range)
                    };
                    for item in items {
                        let (key, _) = item?;
                        let sequence = key.value().1;
                        let bytes = logs
                            .get(sequence)?
                            .context("Core log level index has no record")?;
                        if consume(sequence, bytes.value())? {
                            return Ok(true);
                        }
                    }
                } else {
                    let range = logs.range((lower, upper))?;
                    let items: Box<dyn Iterator<Item = _>> = if backwards {
                        Box::new(range.rev())
                    } else {
                        Box::new(range)
                    };
                    for item in items {
                        let (key, bytes) = item?;
                        if consume(key.value(), bytes.value())? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            })?;
            if stopped {
                break;
            }
        }
        if backwards {
            page.rows.reverse();
        }
        Ok(page)
    }

    fn detail_record(&self, cursor: CoreLogCursor) -> Result<CoreLogRecord> {
        self.validate_cursor(&cursor)
            .map_err(|_| CoreLogError::RecordGone)?;
        let index = self
            .segments
            .iter()
            .position(|s| s.number == cursor.segment)
            .unwrap();
        self.with_read(index, |transaction| {
            let table = transaction.open_table(LOGS)?;
            let value = table
                .get(cursor.sequence)?
                .ok_or(CoreLogError::RecordGone)?;
            ensure!(
                value.value().len() <= MAX_RECORD_BYTES,
                "Core log record exceeds its size limit"
            );
            Ok(serde_json::from_slice(value.value())?)
        })
    }

    fn clear_store(&mut self) -> Result<()> {
        drop(self.active.take());
        self.uncertain = None;
        self.control.generation = uuid::Uuid::new_v4().to_string();
        self.clearing = true;
        self.finish_clear()
    }

    fn finish_clear(&mut self) -> Result<()> {
        self.write_control()?;
        while let Some(segment) = self.segments.first() {
            fs::remove_file(&segment.path)?;
            self.segments.remove(0);
        }
        self.create_segment()?;
        self.clearing = false;
        Ok(())
    }
}

impl CoreLogStore for RedbCoreLogStore {
    fn append(&mut self, records: &[Vec<u8>]) -> CoreLogResult<()> {
        self.append_batch(records).map_err(map_error)
    }
    fn query(&mut self, query: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.query_page(query).map_err(map_error)
    }
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.detail_record(cursor).map_err(map_error)
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        self.clear_store().map_err(map_error)
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        self.current_status().map_err(map_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_shard_creation_leaves_no_orphan_database() {
        let directory = tempfile::TempDir::new().unwrap();
        assert!(
            RedbCoreLogStore::open_with_limits(
                directory.path().into(),
                Limits {
                    total: 1024 * 1024,
                    segment: 16 * 1024
                }
            )
            .is_err()
        );
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "redb")
        }));
        let store = RedbCoreLogStore::open(directory.path().into()).unwrap();
        assert!(store.current_status().unwrap().head.is_none());
    }

    #[test]
    fn durable_clear_intent_finishes_after_restart() {
        let directory = tempfile::TempDir::new().unwrap();
        let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
        let old = store.control.generation.clone();
        let path = store.segments[0].path.clone();
        drop(store.active.take());
        store.control.generation = uuid::Uuid::new_v4().to_string();
        store.write_control().unwrap();
        drop(store);
        assert!(path.exists());
        let store = RedbCoreLogStore::open(directory.path().into()).unwrap();
        assert!(!path.exists());
        let status = store.current_status().unwrap();
        assert_ne!(status.generation, old);
        assert!(status.head.is_none());
        assert!(status.bytes <= status.budget);
    }

    #[test]
    fn retry_recovers_a_commit_even_after_reopening_temporarily_fails() {
        for committed in [false, true] {
            let directory = tempfile::TempDir::new().unwrap();
            let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
            let record = |number| {
                serde_json::to_vec(&CoreLogRecord {
                    source: CoreLogSource {
                        capture: "capture".into(),
                        instance_id: "instance".into(),
                        core_kind: None,
                    },
                    received_at: number,
                    time: None,
                    log_type: "debug".into(),
                    payload: number.to_string(),
                })
                .unwrap()
            };
            store.append(&[record(1)]).unwrap();
            let pending = vec![record(2)];
            if committed {
                store.append(&pending).unwrap();
            }
            store.uncertain = Some((1, 1));
            drop(store.active.take());
            let path = store.segments.last().unwrap().path.clone();
            let unavailable = directory.path().join("withheld");
            fs::rename(&path, &unavailable).unwrap();
            assert!(store.append(&pending).is_err());
            assert!(store.uncertain.is_some());
            fs::rename(unavailable, path).unwrap();
            store.append(&pending).unwrap();
            let page = store
                .query(CoreLogQuery {
                    direction: CoreLogDirection::Latest,
                    cursor: None,
                    level: None,
                    keyword: String::new(),
                    limit: 200,
                })
                .unwrap();
            assert_eq!(page.rows.len(), 2);
            assert_eq!(page.status.head.unwrap().sequence, 2);
            assert_eq!(page.rows[1].record.payload, "2");
        }
    }
}
