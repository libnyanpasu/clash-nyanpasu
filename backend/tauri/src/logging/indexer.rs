use std::{io::SeekFrom, ops::Range};

use anyhow::Context;
use camino::Utf8PathBuf;
use chrono::{DateTime, Local};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use specta::Type;
use surrealdb::{RecordId, Surreal, engine::local::Db};

use surrealdb::engine::local::Mem;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, Hash, Eq, PartialEq)]
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

struct LogIndex {
    connection: Surreal<Db>,
    table_name: String,
}

impl LogIndex {
    pub async fn try_new(table_name: String) -> anyhow::Result<Self> {
        let connection = Surreal::new::<Mem>(())
            .await
            .context("failed to create connection")?;
        connection
            .use_ns(super::LOGGING_NS)
            .use_db(super::LOGGING_NS)
            .await
            .context("failed to use namespace and database")?;

        let index = Self {
            connection,
            table_name,
        };

        index.create_table().await?;

        Ok(index)
    }

    async fn create_table(&self) -> anyhow::Result<()> {
        let table_name = self.table_name.as_str();
        let sql = format!(
            r#"DEFINE TABLE {table_name} TYPE NORMAL SCHEMAFULL PERMISSIONS NONE;

-- ------------------------------
-- FIELDS
-- ------------------------------ 

DEFINE FIELD end_pos ON {table_name} TYPE int PERMISSIONS FULL;
DEFINE FIELD level ON {table_name} TYPE string PERMISSIONS FULL;
DEFINE FIELD line_number ON {table_name} TYPE int PERMISSIONS FULL;
DEFINE FIELD start_pos ON {table_name} TYPE int PERMISSIONS FULL;
DEFINE FIELD target ON {table_name} TYPE string PERMISSIONS FULL;
DEFINE FIELD timestamp ON {table_name} TYPE int PERMISSIONS FULL;

-- ------------------------------
-- INDEXES
-- ------------------------------
DEFINE INDEX line_numberIndex ON TABLE {table_name} FIELDS line_number UNIQUE;
DEFINE INDEX levelIndex ON TABLE {table_name} FIELDS level;
DEFINE INDEX timestampIndex ON TABLE {table_name} FIELDS timestamp;
DEFINE INDEX targetIndex ON TABLE {table_name} FIELDS target;
DEFINE INDEX timestampAndLevel ON {table_name} FIELDS timestamp, level;
            "#,
        );
        self.connection
            .query(sql)
            .await
            .context("failed to create table")?;
        Ok(())
    }

    pub async fn add_entry(&self, entry: LogEntry) -> anyhow::Result<()> {
        #[derive(Debug, Serialize, Deserialize)]
        struct Record {
            id: RecordId,
        }

        let _: Option<Record> = self
            .connection
            .create(&self.table_name)
            .content(entry)
            .await
            .context("failed to add entry")?;
        Ok(())
    }

    fn build_query(&self, query: Query) -> String {
        let table_name = self.table_name.as_str();
        let offset = query.offset;
        let limit = query.limit;
        let mut sql = format!("SELECT * FROM {table_name} WHERE line_number >= {offset}");
        if let Some(level) = query.level {
            let level = level
                .iter()
                .map(|l| format!("level = {}", serde_json::to_string(l).unwrap()))
                .collect::<Vec<_>>()
                .join(" OR ");
            sql = format!("{sql} AND ({level})");
        }
        if let Some(target) = query.target {
            let target = target
                .iter()
                .map(|t| format!("target = {}", serde_json::to_string(t).unwrap()))
                .collect::<Vec<_>>()
                .join(" OR ");
            sql = format!("{sql} AND ({target})");
        }
        if let Some(timestamp) = query.timestamp {
            let start = timestamp.start;
            let end = timestamp.end;
            let timestamp = format!("{}..{}", start, end);
            sql = format!("{sql} AND timestamp IN {timestamp}");
        }

        format!("{sql} ORDER BY line_number ASC LIMIT {limit}")
    }

    fn explain_query(&self, sql: String) -> String {
        format!("{sql} EXPLAIN FULL")
    }

    pub async fn query(&self, query: Query) -> anyhow::Result<Vec<LogEntry>> {
        let sql = self.build_query(query);

        #[cfg(debug_assertions)]
        dbg!(&sql);

        let mut res = self
            .connection
            .query(sql)
            .await
            .context("failed to query")?;
        let results: Vec<LogEntry> = res.take(0).context("failed to take results")?;
        Ok(results)
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
    pub async fn try_new(path: Utf8PathBuf) -> anyhow::Result<Self> {
        let index = LogIndex::try_new(format!(
            "{}_{}",
            super::LOGGING_DB_PREFIX,
            path.file_name()
                .unwrap()
                .replace(".", "_")
                .replace(" ", "_")
                .replace("-", "__")
        ))
        .await
        .context("failed to create index")?;

        Ok(Self {
            index,
            path,
            current: CurrentPos::default(),
        })
    }

    async fn handle_line(
        &mut self,
        line: &str,
        current: &mut CurrentPos,
        bytes_read: usize,
    ) -> anyhow::Result<()> {
        let tracing_json: TracingJson = serde_json::from_str(line)?;
        let end_pos = current.end_pos + bytes_read;
        let entry = LogEntry {
            line_number: current.line,
            level: tracing_json.level,
            timestamp: tracing_json.timestamp.timestamp_millis() as u64,
            target: tracing_json.target,
            start_pos: current.end_pos,
            end_pos,
        };
        self.index.add_entry(entry).await?;
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
            self.handle_line(&line, &mut current, bytes_read).await?;
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
            self.handle_line(&line, &mut current, bytes_read).await?;
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
    use tokio::fs;

    #[tokio::test]
    async fn test_log_index() {
        let index = LogIndex::try_new("test".to_string()).await.unwrap();
        let query = QueryBuilder::default().build().unwrap();
        let results = index.query(query).await.unwrap();
        assert!(results.is_empty(), "results should be empty");

        let entry = LogEntry {
            line_number: 1,
            level: LoggingLevel::INFO,
            timestamp: 1740504078324,
            target: "test".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry).await.unwrap();

        let entry = LogEntry {
            line_number: 2,
            level: LoggingLevel::WARN,
            timestamp: 1740417699000,
            target: "test".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry).await.unwrap();

        let entry = LogEntry {
            line_number: 3,
            level: LoggingLevel::ERROR,
            timestamp: 1740417699000,
            target: "test".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry).await.unwrap();

        let entry = LogEntry {
            line_number: 4,
            level: LoggingLevel::INFO,
            timestamp: 1740331299000,
            target: "different_target".to_string(),
            start_pos: 0,
            end_pos: 0,
        };
        index.add_entry(entry).await.unwrap();

        // Test offset limit
        let query = QueryBuilder::default().offset(1).limit(1).build().unwrap();
        let results = index.query(query).await.unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 1, "results should have 1 entries");
        assert_eq!(results[0].line_number, 1);

        // Test filter by level
        let query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO])
            .build()
            .unwrap();
        let results = index.query(query).await.unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 2, "results should have 2 entries");
        assert_eq!(results[0].line_number, 1);
        assert_eq!(results[1].line_number, 4);

        let query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO, LoggingLevel::WARN])
            .build()
            .unwrap();
        let results = index.query(query).await.unwrap();
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
        let results = index.query(query).await.unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 3, "results should have 2 entries");
        assert_eq!(results[0].line_number, 1);
        assert_eq!(results[1].line_number, 2);
        assert_eq!(results[2].line_number, 3);

        // test filter by timestamp
        let query = QueryBuilder::default()
            .timestamp(1740417699000..1740504078324)
            .build()
            .unwrap();
        let results = index.query(query).await.unwrap();
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
        let results = index.query(query).await.unwrap();
        dbg!(&results);
        assert_eq!(results.len(), 1, "results should have 1 entries");
        assert_eq!(results[0].line_number, 2);
    }

    async fn create_test_log_file(
        entries: Vec<&str>,
    ) -> anyhow::Result<(NamedTempFile, Utf8PathBuf)> {
        let mut file = NamedTempFile::new()?;
        for entry in entries {
            writeln!(file, "{}", entry)?;
        }
        file.flush()?;

        let path = file.path().to_str().unwrap().to_string();
        let utf8_path = Utf8PathBuf::from(path);

        Ok((file, utf8_path))
    }

    async fn append_to_log_file(
        file: &mut NamedTempFile,
        entries: Vec<&str>,
    ) -> anyhow::Result<()> {
        for entry in entries {
            writeln!(file, "{}", entry)?;
        }
        file.flush()?;
        Ok(())
    }

    async fn get_sample_log_entries() -> Vec<&'static str> {
        vec![
            r#"{"level":"INFO","target":"app::module1","timestamp":"2023-02-25T10:15:30+00:00"}"#,
            r#"{"level":"WARN","target":"app::module2","timestamp":"2023-02-25T10:16:30+00:00"}"#,
            r#"{"level":"ERROR","target":"app::module1","timestamp":"2023-02-25T10:17:30+00:00"}"#,
            r#"{"level":"DEBUG","target":"app::module3","timestamp":"2023-02-25T10:18:30+00:00"}"#,
        ]
    }

    async fn get_additional_log_entries() -> Vec<&'static str> {
        vec![
            r#"{"level":"INFO","target":"app::module2","timestamp":"2023-02-25T10:19:30+00:00"}"#,
            r#"{"level":"FATAL","target":"app::module1","timestamp":"2023-02-25T10:20:30+00:00"}"#,
        ]
    }

    #[tokio::test]
    async fn test_indexer_creation() -> anyhow::Result<()> {
        let entries = get_sample_log_entries().await;
        let (_guard, path) = create_test_log_file(entries).await?;

        let indexer = Indexer::try_new(path).await?;
        assert!(indexer.current.line == 0, "Initial line count should be 0");
        assert!(
            indexer.current.end_pos == 0,
            "Initial end position should be 0"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_build_index() -> anyhow::Result<()> {
        let entries = get_sample_log_entries().await;
        let (_guard, path) = create_test_log_file(entries.clone()).await?;

        let mut indexer = Indexer::try_new(path.clone()).await?;
        indexer.build_index().await?;

        // Verify that all entries were indexed
        assert_eq!(
            indexer.current.line,
            entries.len() as u64,
            "Line count should match number of entries"
        );

        // Query the index to verify entries
        let query = QueryBuilder::default().build()?;
        let results = indexer.index.query(query).await?;

        assert_eq!(
            results.len(),
            entries.len(),
            "Query should return all indexed entries"
        );

        // Verify specific entries by level
        let info_query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO])
            .build()?;
        let info_results = indexer.index.query(info_query).await?;
        assert_eq!(info_results.len(), 1, "Should have 1 INFO entry");

        let warn_query = QueryBuilder::default()
            .level(vec![LoggingLevel::WARN])
            .build()?;
        let warn_results = indexer.index.query(warn_query).await?;
        assert_eq!(warn_results.len(), 1, "Should have 1 WARN entry");

        let error_query = QueryBuilder::default()
            .level(vec![LoggingLevel::ERROR])
            .build()?;
        let error_results = indexer.index.query(error_query).await?;
        assert_eq!(error_results.len(), 1, "Should have 1 ERROR entry");

        let debug_query = QueryBuilder::default()
            .level(vec![LoggingLevel::DEBUG])
            .build()?;
        let debug_results = indexer.index.query(debug_query).await?;
        assert_eq!(debug_results.len(), 1, "Should have 1 DEBUG entry");

        Ok(())
    }

    #[tokio::test]
    async fn test_on_file_change() -> anyhow::Result<()> {
        let initial_entries = get_sample_log_entries().await;
        let (mut file, path) = create_test_log_file(initial_entries.clone()).await?;

        // Initialize and build the initial index
        let mut indexer = Indexer::try_new(path.clone()).await?;
        indexer.build_index().await?;

        // Verify initial indexing
        assert_eq!(
            indexer.current.line,
            initial_entries.len() as u64,
            "Line count should match initial entries"
        );

        // Add more entries to the file
        let additional_entries = get_additional_log_entries().await;
        append_to_log_file(&mut file, additional_entries.clone()).await?;

        // Process file changes
        indexer.on_file_change().await?;

        // Verify that all entries are now indexed
        let total_entries = initial_entries.len() + additional_entries.len();
        assert_eq!(
            indexer.current.line, total_entries as u64,
            "Line count should match total entries"
        );

        // Query all entries
        let query = QueryBuilder::default().build()?;
        let results = indexer.index.query(query).await?;
        assert_eq!(
            results.len(),
            total_entries,
            "Query should return all indexed entries"
        );

        // Check for specific new entry
        let fatal_query = QueryBuilder::default()
            .level(vec![LoggingLevel::FATAL])
            .build()?;
        let fatal_results = indexer.index.query(fatal_query).await?;
        assert_eq!(
            fatal_results.len(),
            1,
            "Should have 1 FATAL entry from file change"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_indexer_with_target_filter() -> anyhow::Result<()> {
        let entries = get_sample_log_entries().await;
        let (_guard, path) = create_test_log_file(entries).await?;

        let mut indexer = Indexer::try_new(path).await?;
        indexer.build_index().await?;

        // Query by target
        let target_query = QueryBuilder::default()
            .target(vec!["app::module1".to_string()])
            .build()?;
        let target_results = indexer.index.query(target_query).await?;

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
        let entries = get_sample_log_entries().await;
        let additional_entries = get_additional_log_entries().await;
        let mut all_entries = entries.clone();
        all_entries.extend(additional_entries.clone());

        let (_guard, path) = create_test_log_file(all_entries).await?;

        let mut indexer = Indexer::try_new(path).await?;
        indexer.build_index().await?;

        // Complex query with multiple filters
        let complex_query = QueryBuilder::default()
            .level(vec![LoggingLevel::INFO, LoggingLevel::WARN])
            .target(vec!["app::module2".to_string()])
            .build()?;

        let complex_results = indexer.index.query(complex_query).await?;
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
