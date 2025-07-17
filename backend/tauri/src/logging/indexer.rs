use std::{
    collections::{BTreeMap, BTreeSet},
    io::SeekFrom,
    ops::Range,
};

use anyhow::Context;
use bumpalo::Bump;
use camino::Utf8PathBuf;
use chrono::{DateTime, Local};
use derive_builder::Builder;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Type, Hash, Eq, PartialEq, Ord, PartialOrd,
)]
#[serde(rename_all = "UPPERCASE")]
#[allow(clippy::upper_case_acronyms)]
pub enum LoggingLevel {
    DEBUG,
    INFO,
    WARN,
    ERROR,
    FATAL,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct LogEntry {
    /// The line number of the log entry.
    /// For query limit, and offset.
    pub line_number: u64,
    /// The level of the log entry.
    pub level: LoggingLevel,
    /// The timestamp of the log entry.
    pub timestamp: u64,
    /// The target of the log entry.
    /// eg: "backend::logging::indexer"
    pub target: String,
    /// The start position of the log entry in the file.
    pub start_pos: usize,
    /// The end position of the log entry in the file.
    pub end_pos: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Type)]
struct CurrentPos {
    line: u64,
    end_pos: usize,
}

#[derive(Debug, Builder, Clone, Serialize, Deserialize, Type)]
pub struct Query {
    #[builder(default)]
    offset: usize,
    #[builder(default = 100)]
    limit: usize,
    #[builder(default, setter(into, strip_option))]
    level: Option<Vec<LoggingLevel>>,
    #[builder(default, setter(into, strip_option))]
    target: Option<Vec<String>>,
    #[builder(default, setter(into, strip_option))]
    timestamp: Option<Range<u64>>,
}

pub type LineNumber = u64;
pub type Timestamp = u64;

struct LogIndex {
    /// a bump allocator for heap allocation
    arena: Bump,

    /// index by line number
    line_index: BTreeMap<LineNumber, *mut LogEntry>,
    /// index by timestamp
    /// in our case, the timestamp is nanoseconds, so only one item per timestamp
    timestamp_index: BTreeMap<Timestamp, LineNumber>,
    /// index by level
    level_index: FxHashMap<LoggingLevel, *mut Vec<LineNumber>>,
    /// index by target
    target_index: FxHashMap<String, *mut Vec<LineNumber>>,

    last_line_number: Option<LineNumber>,
}

impl core::fmt::Debug for LogIndex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let lines = self
            .line_index
            .values()
            .map(|v| unsafe {
                let v = &**v;
                v.clone()
            })
            .collect_vec();
        let levels = BTreeMap::from_iter(self.level_index.iter().map(|(k, v)| {
            (k, unsafe {
                let v = &**v;
                v.clone()
            })
        }));
        let targets = BTreeMap::from_iter(self.target_index.iter().map(|(k, v)| {
            (k, unsafe {
                let v = &**v;
                v.clone()
            })
        }));
        write!(
            f,
            "LogIndex {{ 
                lines: {:?}; 
                timestamp_index: {:?};
                level_index: {:?};
                target_index: {:?};
                last_line_number: {:?} 
            }}",
            lines, self.timestamp_index, levels, targets, self.last_line_number
        )
    }
}

impl LogIndex {
    pub fn new() -> Self {
        Self {
            arena: Bump::new(),
            line_index: BTreeMap::new(),
            timestamp_index: BTreeMap::new(),
            level_index: FxHashMap::default(),
            target_index: FxHashMap::default(),
            last_line_number: None,
        }
    }

    #[inline]
    /// add an entry to the index
    pub fn add_entry(&mut self, entry: LogEntry) {
        let line_number = entry.line_number;
        let timestamp = entry.timestamp;
        let level = entry.level;
        let target = entry.target.clone();

        let entry_ptr = self.arena.alloc(entry) as *mut LogEntry;
        // update level index
        {
            let entry = self.level_index.entry(level);
            entry
                .and_modify(|v| {
                    // SAFETY: we are sure that the vec_ptr is valid
                    unsafe {
                        let v = &mut **v;
                        v.push(line_number);
                    }
                })
                .or_insert_with(|| {
                    let vec = self.arena.alloc(vec![line_number]);
                    vec as *mut Vec<u64>
                });
        }
        // update timestamp index
        {
            let entry = self.timestamp_index.entry(timestamp);
            entry
                .and_modify(|v| {
                    tracing::warn!(
                        "duplicate timestamp: {}; previous: {}, new: {}",
                        timestamp,
                        v,
                        line_number
                    );
                    *v = line_number;
                })
                .or_insert(line_number);
        }
        // update target index
        {
            let entry = self.target_index.entry(target);
            entry
                .and_modify(|v| {
                    // SAFETY: we are sure that the vec_ptr is valid
                    unsafe {
                        let v = &mut **v;
                        v.push(line_number);
                    }
                })
                .or_insert_with(|| {
                    let vec = self.arena.alloc(vec![line_number]);
                    vec as *mut Vec<u64>
                });
        }
        // update line index
        {
            self.line_index.insert(line_number, entry_ptr);
        }

        self.last_line_number = Some(line_number);
    }

    // TODO: optimize query performance
    pub fn query(&self, query: Query) -> Option<Vec<LogEntry>> {
        // query by timestamp
        let mut matching_lines: Option<Vec<LineNumber>> = None;
        if let Some(range) = query.timestamp {
            let mut range = self.timestamp_index.range(range);
            let (_, start) = range.next()?;
            let end = match range.last() {
                Some((_, end_line)) => *end_line,
                None => *start,
            };
            matching_lines = Some(Vec::from_iter(*start..=end));
        }

        // query by level
        if let Some(levels) = query.level {
            let mut matched_lines = BTreeSet::new();
            for level in levels {
                if let Some(lines) = self.level_index.get(&level) {
                    // SAFETY: we have allocated the vec on the heap by bumpalo
                    unsafe {
                        let lines = &**lines;
                        matched_lines.extend(lines.iter());
                    }
                }
            }
            matching_lines = match matching_lines {
                Some(lines) => Some(
                    lines
                        .into_iter()
                        .filter(|line| matched_lines.contains(line))
                        .collect_vec(),
                ),
                None => Some(matched_lines.into_iter().collect_vec()),
            }
        }

        // query by target
        if let Some(targets) = query.target {
            let mut matched_lines = BTreeSet::new();
            for target in targets {
                if let Some(lines) = self.target_index.get(&target) {
                    // SAFETY: we have allocated the vec on the heap by bumpalo
                    unsafe {
                        let lines = &**lines;
                        matched_lines.extend(lines.iter());
                    }
                }
            }
            matching_lines = match matching_lines {
                Some(lines) => Some(
                    lines
                        .into_iter()
                        .filter(|line| matched_lines.contains(line))
                        .collect_vec(),
                ),
                None => Some(matched_lines.into_iter().collect_vec()),
            }
        }

        let matching_lines = match matching_lines {
            Some(lines) if lines.is_empty() => return None,
            None => {
                let last_line = self.last_line_number.as_ref()?;
                Vec::from_iter(0..=*last_line)
            }
            Some(lines) => lines,
        };

        #[cfg(test)]
        dbg!(&matching_lines);

        let results = matching_lines
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            // SAFETY: we are sure that the line_index is valid, which is allocated by bumpalo,
            // and the pool only be dropped when this index is dropped
            .map(|line_number| unsafe {
                let entry = &**self.line_index.get(&line_number).unwrap();
                entry.clone()
            })
            .collect_vec();

        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    }
}

pub struct Indexer {
    index: LogIndex,
    path: Utf8PathBuf,
    current: CurrentPos,
}

#[derive(Debug, Serialize, Deserialize)]
struct TracingJson {
    level: LoggingLevel,
    target: String,
    timestamp: DateTime<Local>,
}

impl Indexer {
    pub fn new(path: Utf8PathBuf) -> Self {
        Self {
            index: LogIndex::new(),
            path,
            current: CurrentPos::default(),
        }
    }

    fn handle_line(
        &mut self,
        line: &str,
        current: &mut CurrentPos,
        bytes_read: usize,
    ) -> anyhow::Result<()> {
        let tracing_json: TracingJson =
            serde_json::from_str(line).context("failed to parse log line")?;
        let end_pos = current.end_pos + bytes_read;
        let entry = LogEntry {
            line_number: current.line,
            level: tracing_json.level,
            timestamp: tracing_json.timestamp.timestamp_millis() as u64,
            target: tracing_json.target,
            start_pos: current.end_pos,
            end_pos,
        };
        self.index.add_entry(entry);
        current.line += 1;
        current.end_pos = end_pos;
        Ok(())
    }

    pub async fn build_index(&mut self) -> anyhow::Result<()> {
        // read file line by line
        let mut file = tokio::fs::File::open(&self.path).await?;
        let mut reader = BufReader::new(&mut file);
        let mut current = CurrentPos::default();

        let mut line = String::new();
        loop {
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                break;
            }
            self.handle_line(&line, &mut current, bytes_read)?;
            line.clear();
        }
        #[cfg(test)]
        {
            let bytes_count = file.metadata().await?.len();
            pretty_assertions::assert_eq!(bytes_count, current.end_pos as u64);
        }
        self.current = current;
        Ok(())
    }

    pub fn query(&self, query: Query) -> Option<Vec<LogEntry>> {
        self.index.query(query)
    }

    pub async fn on_file_change(&mut self) -> anyhow::Result<()> {
        let mut file = tokio::fs::File::open(&self.path).await?;
        file.seek(SeekFrom::Start(self.current.end_pos as u64))
            .await?;
        let mut reader = BufReader::new(file);
        let mut current = std::mem::take(&mut self.current);
        let mut line = String::new();
        loop {
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                break;
            }
            self.handle_line(&line, &mut current, bytes_read)?;
            line.clear();
        }
        self.current = current;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_log_index() {
        let mut index = LogIndex::new();
        let query = QueryBuilder::default().build().unwrap();
        let results = index.query(query);
        assert!(results.is_none(), "results should be empty");

        let entry = LogEntry {
            line_number: 1,
            level: LoggingLevel::INFO,
            timestamp: 1740504078324,
            target: "test".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry);

        let entry = LogEntry {
            line_number: 2,
            level: LoggingLevel::WARN,
            timestamp: 1740417699000,
            target: "test".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry);

        let entry = LogEntry {
            line_number: 3,
            level: LoggingLevel::ERROR,
            timestamp: 1740417699001,
            target: "test".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry);

        let entry = LogEntry {
            line_number: 4,
            level: LoggingLevel::INFO,
            timestamp: 1740331299000,
            target: "different_target".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry);

        dbg!(&index);

        // Test offset limit
        let query = QueryBuilder::default().offset(1).limit(1).build().unwrap();
        let results = index.query(query).unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 1, "results should have 1 entries");
        assert_eq!(results[0].line_number, 1);

        // Test filter by level
        let query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO])
            .build()
            .unwrap();
        let results = index.query(query).unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 2, "results should have 2 entries");
        assert_eq!(results[0].line_number, 1);
        assert_eq!(results[1].line_number, 4);

        let query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO, LoggingLevel::WARN])
            .build()
            .unwrap();
        let results = index.query(query).unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 3, "results should have 3 entries");
        assert_eq!(results[0].line_number, 1);
        assert_eq!(results[1].line_number, 2);
        assert_eq!(results[2].line_number, 4);

        // test filter by target
        let query = QueryBuilder::default()
            .target(vec!["test".to_string()])
            .build()
            .unwrap();
        let results = index.query(query).unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 3, "results should have 3 entries");
        assert_eq!(results[0].line_number, 1);
        assert_eq!(results[1].line_number, 2);
        assert_eq!(results[2].line_number, 3);

        // test filter by timestamp
        let query = QueryBuilder::default()
            .timestamp(1740417699000..1740504078324)
            .build()
            .unwrap();
        let results = index.query(query).unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 2, "results should have 2 entries");
        assert_eq!(results[0].line_number, 2);
        assert_eq!(results[1].line_number, 3);

        // a complex query
        let query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO, LoggingLevel::WARN])
            .target(vec!["test".to_string()])
            .timestamp(1740417699000..1740504078324)
            .build()
            .unwrap();
        let results = index.query(query).unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 1, "results should have 1 entries");
        assert_eq!(results[0].line_number, 2);
    }

    fn create_test_log_file(entries: Vec<&str>) -> anyhow::Result<(NamedTempFile, Utf8PathBuf)> {
        let mut file = NamedTempFile::new()?;
        for entry in entries {
            writeln!(file, "{entry}")?;
        }
        file.flush()?;

        let path = file.path().to_str().unwrap().to_string();
        let utf8_path = Utf8PathBuf::from(path);

        Ok((file, utf8_path))
    }

    fn append_to_log_file(file: &mut NamedTempFile, entries: Vec<&str>) -> anyhow::Result<()> {
        for entry in entries {
            writeln!(file, "{entry}")?;
        }
        file.flush()?;
        Ok(())
    }

    fn get_sample_log_entries() -> Vec<&'static str> {
        vec![
            r#"{"level":"INFO","target":"app::module1","timestamp":"2023-02-25T10:15:30+00:00"}"#,
            r#"{"level":"WARN","target":"app::module2","timestamp":"2023-02-25T10:16:30+00:00"}"#,
            r#"{"level":"ERROR","target":"app::module1","timestamp":"2023-02-25T10:17:30+00:00"}"#,
            r#"{"level":"DEBUG","target":"app::module3","timestamp":"2023-02-25T10:18:30+00:00"}"#,
        ]
    }

    fn get_additional_log_entries() -> Vec<&'static str> {
        vec![
            r#"{"level":"INFO","target":"app::module2","timestamp":"2023-02-25T10:19:30+00:00"}"#,
            r#"{"level":"FATAL","target":"app::module1","timestamp":"2023-02-25T10:20:30+00:00"}"#,
        ]
    }

    #[test]
    fn test_indexer_creation() {
        let entries = get_sample_log_entries();
        let (_guard, path) = create_test_log_file(entries).unwrap();

        let indexer = Indexer::new(path);
        assert!(indexer.current.line == 0, "Initial line count should be 0");
        assert!(
            indexer.current.end_pos == 0,
            "Initial end position should be 0"
        );
    }

    #[tokio::test]
    async fn test_build_index() -> anyhow::Result<()> {
        let entries = get_sample_log_entries();
        let (_guard, path) = create_test_log_file(entries.clone()).unwrap();

        let mut indexer = Indexer::new(path);
        indexer.build_index().await.unwrap();

        // Verify that all entries were indexed
        assert_eq!(
            indexer.current.line,
            entries.len() as u64,
            "Line count should match number of entries"
        );

        // Query the index to verify entries
        let query = QueryBuilder::default().build().unwrap();
        let results = indexer.index.query(query).unwrap();

        assert_eq!(
            results.len(),
            entries.len(),
            "Query should return all indexed entries"
        );

        // Verify specific entries by level
        let info_query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO])
            .build()?;
        let info_results = indexer.index.query(info_query).unwrap();
        assert_eq!(info_results.len(), 1, "Should have 1 INFO entry");

        let warn_query = QueryBuilder::default()
            .level(vec![LoggingLevel::WARN])
            .build()?;
        let warn_results = indexer.index.query(warn_query).unwrap();
        assert_eq!(warn_results.len(), 1, "Should have 1 WARN entry");

        let error_query = QueryBuilder::default()
            .level(vec![LoggingLevel::ERROR])
            .build()?;
        let error_results = indexer.index.query(error_query).unwrap();
        assert_eq!(error_results.len(), 1, "Should have 1 ERROR entry");

        let debug_query = QueryBuilder::default()
            .level(vec![LoggingLevel::DEBUG])
            .build()?;
        let debug_results = indexer.index.query(debug_query).unwrap();
        assert_eq!(debug_results.len(), 1, "Should have 1 DEBUG entry");

        Ok(())
    }

    #[tokio::test]
    async fn test_on_file_change() -> anyhow::Result<()> {
        let initial_entries = get_sample_log_entries();
        let (mut file, path) = create_test_log_file(initial_entries.clone()).unwrap();

        // Initialize and build the initial index
        let mut indexer = Indexer::new(path);
        indexer.build_index().await.unwrap();

        // Verify initial indexing
        assert_eq!(
            indexer.current.line,
            initial_entries.len() as u64,
            "Line count should match initial entries"
        );

        // Add more entries to the file
        let additional_entries = get_additional_log_entries();
        append_to_log_file(&mut file, additional_entries.clone()).unwrap();

        // Process file changes
        indexer.on_file_change().await?;

        // Verify that all entries are now indexed
        let total_entries = initial_entries.len() + additional_entries.len();
        assert_eq!(
            indexer.current.line, total_entries as u64,
            "Line count should match total entries"
        );

        // Query all entries
        let query = QueryBuilder::default().build().unwrap();
        let results = indexer.index.query(query).unwrap();
        assert_eq!(
            results.len(),
            total_entries,
            "Query should return all indexed entries"
        );

        // Check for specific new entry
        let fatal_query = QueryBuilder::default()
            .level(vec![LoggingLevel::FATAL])
            .build()?;
        let fatal_results = indexer.index.query(fatal_query).unwrap();
        assert_eq!(
            fatal_results.len(),
            1,
            "Should have 1 FATAL entry from file change"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_indexer_with_target_filter() -> anyhow::Result<()> {
        let entries = get_sample_log_entries();
        let (_guard, path) = create_test_log_file(entries).unwrap();

        let mut indexer = Indexer::new(path);
        indexer.build_index().await.unwrap();

        // Query by target
        let target_query = QueryBuilder::default()
            .target(vec!["app::module1".to_string()])
            .build()?;
        let target_results = indexer.index.query(target_query).unwrap();

        assert_eq!(
            target_results.len(),
            2,
            "Should have 2 entries for app::module1"
        );

        // Verify the levels of the filtered results
        let has_info = target_results.iter().any(|e| e.level == LoggingLevel::INFO);
        let has_error = target_results
            .iter()
            .any(|e| e.level == LoggingLevel::ERROR);

        assert!(has_info, "app::module1 should have an INFO entry");
        assert!(has_error, "app::module1 should have an ERROR entry");

        Ok(())
    }

    #[tokio::test]
    async fn test_indexer_complex_query() -> anyhow::Result<()> {
        let entries = get_sample_log_entries();
        let additional_entries = get_additional_log_entries();
        let mut all_entries = entries.clone();
        all_entries.extend(additional_entries.clone());

        let (_guard, path) = create_test_log_file(all_entries).unwrap();

        let mut indexer = Indexer::new(path);
        indexer.build_index().await.unwrap();

        // Complex query with multiple filters
        let complex_query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO, LoggingLevel::WARN])
            .target(vec!["app::module2".to_string()])
            .build()?;

        let complex_results = indexer.index.query(complex_query).unwrap();
        assert_eq!(
            complex_results.len(),
            2,
            "Complex query should return 2 entries"
        );

        // Verify specific entries
        let has_info = complex_results
            .iter()
            .any(|e| e.level == LoggingLevel::INFO);
        let has_warn = complex_results
            .iter()
            .any(|e| e.level == LoggingLevel::WARN);

        assert!(
            has_info,
            "Results should include INFO entry for app::module2"
        );
        assert!(
            has_warn,
            "Results should include WARN entry for app::module2"
        );

        Ok(())
    }
}
