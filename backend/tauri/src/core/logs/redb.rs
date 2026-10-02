use std::{
    fs::{self, File, OpenOptions},
    ops::Bound,
    path::PathBuf,
};

use anyhow::{Context, Result, bail, ensure};
use redb::{Database, ReadableDatabase, TableDefinition};

use super::{model::*, ports::CoreLogStore};

const LOGS: TableDefinition<u64, &[u8]> = TableDefinition::new("logs");
const LEVELS: TableDefinition<(u8, u64), u8> = TableDefinition::new("levels");
const CACHE_BYTES: usize = 1024 * 1024;

/// The actor owns one lazily opened database for the current Core session.
pub struct RedbCoreLogStore {
    directory: PathBuf,
    database: Option<Database>,
    // Keep deletion and lazy creation exclusive across application processes.
    _lock: File,
    generation: String,
    head: u64,
}

fn map_error(error: anyhow::Error) -> CoreLogError {
    match error.downcast::<CoreLogError>() {
        Ok(error) => error,
        Err(error) => CoreLogError::Unavailable(format!("{error:#}")),
    }
}

impl RedbCoreLogStore {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("owner.lock"))?;
        lock.try_lock()
            .context("Core log directory already owned")?;
        // Remove only our previous session's files, including shards written
        // by the earlier implementation.
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let old_shard = name
                .strip_suffix(".redb")
                .and_then(|stem| stem.rsplit_once('.'))
                .is_some_and(|(generation, sequence)| {
                    uuid::Uuid::parse_str(generation).is_ok() && sequence.parse::<u64>().is_ok()
                });
            if matches!(name, "current.redb" | "control.json" | "control.tmp") || old_shard {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(Self {
            directory,
            database: None,
            _lock: lock,
            generation: uuid::Uuid::new_v4().to_string(),
            head: 0,
        })
    }

    fn cursor(&self, sequence: u64) -> CoreLogCursor {
        CoreLogCursor {
            generation: self.generation.clone(),
            sequence,
        }
    }

    fn current_status(&self) -> Result<CoreLogStatus> {
        let bytes = match fs::metadata(self.directory.join("current.redb")) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        Ok(CoreLogStatus {
            generation: self.generation.clone(),
            first: (self.head > 0).then(|| self.cursor(1)),
            head: (self.head > 0).then(|| self.cursor(self.head)),
            bytes,
            ..Default::default()
        })
    }

    fn append_batch(&mut self, records: &[Vec<u8>]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        if self.database.is_none() {
            let mut builder = Database::builder();
            builder.set_cache_size(CACHE_BYTES);
            self.database = Some(builder.create(self.directory.join("current.redb"))?);
        }
        let transaction = self.database.as_ref().unwrap().begin_write()?;
        let mut last = self.head;
        {
            let mut logs = transaction.open_table(LOGS)?;
            let mut levels = transaction.open_table(LEVELS)?;
            for bytes in records {
                ensure!(bytes.len() <= MAX_RECORD_BYTES, CoreLogError::TooLarge);
                let record: CoreLogRecord = serde_json::from_slice(bytes)?;
                ensure!(record.metadata_fits(), CoreLogError::TooLarge);
                last = last.checked_add(1).context("Core log sequence exhausted")?;
                logs.insert(last, bytes.as_slice())?;
                levels.insert((level_code(&record.log_type), last), 0)?;
            }
        }
        transaction.commit()?;
        self.head = last;
        Ok(())
    }

    fn validate_cursor(&self, cursor: &CoreLogCursor) -> Result<()> {
        if cursor.generation != self.generation
            || cursor.sequence == 0
            || cursor.sequence > self.head
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
        let Some(database) = &self.database else {
            return Ok(page);
        };
        if self.head == 0 {
            return Ok(page);
        }
        let transaction = database.begin_read()?;
        {
            let logs = transaction.open_table(LOGS)?;
            let mut consume = |sequence: u64, bytes: &[u8]| -> Result<bool> {
                ensure!(
                    bytes.len() <= MAX_RECORD_BYTES,
                    "Core log record exceeds its size limit"
                );
                let cursor = self.cursor(sequence);
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
            let lower = if query.cursor.as_ref().is_some() && !backwards {
                Bound::Excluded(query.cursor.as_ref().unwrap().sequence)
            } else {
                Bound::Unbounded
            };
            let upper = if query.cursor.as_ref().is_some() && backwards {
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
                        break;
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
                        break;
                    }
                }
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
        let database = self.database.as_ref().ok_or(CoreLogError::RecordGone)?;
        let transaction = database.begin_read()?;
        let table = transaction.open_table(LOGS)?;
        let value = table
            .get(cursor.sequence)?
            .ok_or(CoreLogError::RecordGone)?;
        ensure!(
            value.value().len() <= MAX_RECORD_BYTES,
            "Core log record exceeds its size limit"
        );
        Ok(serde_json::from_slice(value.value())?)
    }

    fn clear_store(&mut self) -> Result<()> {
        // All transactions end within their store call, before releasing this handle.
        drop(self.database.take());
        self.generation = uuid::Uuid::new_v4().to_string();
        self.head = 0;
        match fs::remove_file(self.directory.join("current.redb")) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
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
