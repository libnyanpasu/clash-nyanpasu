use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    ops::Bound,
    path::PathBuf,
};

use anyhow::{Context, Result, bail, ensure};
use redb::{Database, ReadableDatabase, TableDefinition};

use super::{model::*, ports::CoreLogStore};
use nyanpasu_config::application::CoreLogSettings;

const LOGS: TableDefinition<u64, &[u8]> = TableDefinition::new("logs");
const LEVELS: TableDefinition<(u8, u64), u8> = TableDefinition::new("levels");
const CACHE_BYTES: usize = 1024 * 1024;

struct Shard {
    path: PathBuf,
    first: u64,
    head: u64,
    bytes: u64,
}

/// Only the active shard stays open; sealed shards are opened for a query.
pub struct RedbCoreLogStore {
    directory: PathBuf,
    database: Option<Database>,
    // Keep deletion and lazy creation exclusive across application processes.
    _lock: File,
    generation: String,
    head: u64,
    settings: CoreLogSettings,
    shards: VecDeque<Shard>,
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
            settings: CoreLogSettings::default(),
            shards: VecDeque::new(),
        })
    }

    fn cursor(&self, sequence: u64) -> CoreLogCursor {
        CoreLogCursor {
            generation: self.generation.clone(),
            sequence,
        }
    }

    fn current_status(&self) -> CoreLogStatus {
        CoreLogStatus {
            generation: self.generation.clone(),
            first: self.shards.front().map(|shard| self.cursor(shard.first)),
            head: self.shards.back().map(|shard| self.cursor(shard.head)),
            bytes: self.shards.iter().map(|shard| shard.bytes).sum(),
            ..Default::default()
        }
    }

    fn database_builder() -> redb::Builder {
        let mut builder = Database::builder();
        builder.set_cache_size(CACHE_BYTES);
        builder
    }

    fn rotate(&mut self) -> Result<()> {
        drop(self.database.take());
        if let Some(shard) = self.shards.back_mut() {
            let path = self
                .directory
                .join(format!("{}.{:020}.redb", self.generation, shard.first));
            fs::rename(&shard.path, &path)?;
            shard.path = path;
        }
        Ok(())
    }

    fn evict(&mut self) -> Result<()> {
        let mut bytes: u64 = self.shards.iter().map(|shard| shard.bytes).sum();
        while bytes > self.settings.max_bytes() {
            let shard = self
                .shards
                .front()
                .expect("a nonempty store exceeds its budget");
            if self.shards.len() == 1 {
                drop(self.database.take());
            }
            fs::remove_file(&shard.path)?;
            bytes -= shard.bytes;
            self.shards.pop_front();
        }
        Ok(())
    }

    fn append_batch(&mut self, records: &[PreparedCoreLog]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        if self
            .shards
            .back()
            .is_some_and(|shard| shard.bytes >= self.settings.shard_bytes())
        {
            self.rotate()?;
        }
        if self.database.is_none() {
            let path = self.directory.join("current.redb");
            self.database = Some(Self::database_builder().create(&path)?);
            self.shards.push_back(Shard {
                path,
                first: self.head + 1,
                head: self.head,
                bytes: 0,
            });
        }
        let transaction = self.database.as_ref().unwrap().begin_write()?;
        let mut last = self.head;
        {
            let mut logs = transaction.open_table(LOGS)?;
            let mut levels = transaction.open_table(LEVELS)?;
            for record in records {
                last += 1;
                logs.insert(last, record.bytes.as_slice())?;
                levels.insert((record.level, last), 0)?;
            }
        }
        transaction.commit()?;
        self.head = last;
        let shard = self.shards.back_mut().unwrap();
        shard.head = last;
        shard.bytes = fs::metadata(&shard.path)?.len();
        self.evict()
    }

    fn validate_cursor(&self, cursor: &CoreLogCursor) -> Result<()> {
        if cursor.generation != self.generation
            || self
                .shards
                .front()
                .is_none_or(|shard| cursor.sequence < shard.first)
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
        let status = self.current_status();
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
        let shards: Box<dyn Iterator<Item = &Shard>> = if backwards {
            Box::new(self.shards.iter().rev())
        } else {
            Box::new(self.shards.iter())
        };
        for shard in shards {
            if query.cursor.as_ref().is_some_and(|cursor| {
                if backwards {
                    shard.first >= cursor.sequence
                } else {
                    shard.head <= cursor.sequence
                }
            }) {
                continue;
            }
            let opened;
            let database =
                if shard.path == self.directory.join("current.redb") && self.database.is_some() {
                    self.database.as_ref().unwrap()
                } else {
                    opened = Self::database_builder().open(&shard.path)?;
                    &opened
                };
            let transaction = database.begin_read()?;
            let logs = transaction.open_table(LOGS)?;
            let mut consume = |sequence: u64, bytes: &[u8]| -> Result<bool> {
                ensure!(
                    bytes.len() <= MAX_RECORD_BYTES,
                    "Core log record exceeds its size limit"
                );
                let cursor = self.cursor(sequence);
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
            if page.more {
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
        let shard = self
            .shards
            .iter()
            .find(|shard| cursor.sequence >= shard.first && cursor.sequence <= shard.head)
            .ok_or(CoreLogError::RecordGone)?;
        let opened;
        let database =
            if shard.path == self.directory.join("current.redb") && self.database.is_some() {
                self.database.as_ref().unwrap()
            } else {
                opened = Self::database_builder().open(&shard.path)?;
                &opened
            };
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
        while let Some(shard) = self.shards.front() {
            fs::remove_file(&shard.path)?;
            self.shards.pop_front();
        }
        self.generation = uuid::Uuid::new_v4().to_string();
        self.head = 0;
        Ok(())
    }
}

impl CoreLogStore for RedbCoreLogStore {
    fn configure(&mut self, settings: CoreLogSettings) -> CoreLogResult<()> {
        settings
            .validate()
            .map_err(|reason| CoreLogError::Unavailable(reason.into()))?;
        self.settings = settings;
        Ok(())
    }
    fn append(&mut self, records: &[PreparedCoreLog]) -> CoreLogResult<()> {
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
        Ok(self.current_status())
    }
}
