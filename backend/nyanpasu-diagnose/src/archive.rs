//! Atomic, redacted archives over explicitly supplied paths.
use crate::report::Report;
use nyanpasu_config::redact::redact_text;
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use zip::{ZipWriter, write::SimpleFileOptions};

pub const LOG_CAP: u64 = 16 * 1024 * 1024;
pub const LOG_AGE: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const PREFIX: &str = "clash-nyanpasu-diagnostics-";

pub struct Archive {
    writer: ZipWriter<File>,
    temporary: tempfile::TempPath,
    directory: PathBuf,
    home: Option<String>,
}
impl Archive {
    pub fn new(logs: &Path, home: Option<String>) -> Result<Self, String> {
        let directory = logs.join("diagnostics");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let file = tempfile::Builder::new()
            .prefix(".diagnostics-")
            .tempfile_in(&directory)
            .map_err(|e| e.to_string())?;
        let (file, temporary) = file.into_parts();
        Ok(Self {
            writer: ZipWriter::new(file),
            temporary,
            directory,
            home,
        })
    }
    pub fn write_entry(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .start_file(name, SimpleFileOptions::default())
            .map_err(|e| e.to_string())?;
        self.writer.write_all(bytes).map_err(|e| e.to_string())
    }
    /// Writes `lines`, each ending in a newline, without joining them first.
    pub fn core_logs(
        &mut self,
        lines: &[String],
        fraction: &mut dyn FnMut(f32),
    ) -> Result<(), String> {
        self.writer
            .start_file("logs/core/core.log", SimpleFileOptions::default())
            .map_err(|e| e.to_string())?;
        for (index, line) in lines.iter().enumerate() {
            self.writer
                .write_all(line.as_bytes())
                .map_err(|e| e.to_string())?;
            if index % 1024 == 0 {
                fraction(index as f32 / lines.len() as f32);
            }
        }
        Ok(())
    }
    pub fn app_logs(
        &mut self,
        logs: &Path,
        report: &mut Report,
        fraction: &mut dyn FnMut(f32),
    ) -> Result<(), String> {
        self.app_logs_at(logs, report, fraction, SystemTime::now(), LOG_CAP)
    }
    fn app_logs_at(
        &mut self,
        logs: &Path,
        report: &mut Report,
        fraction: &mut dyn FnMut(f32),
        now: SystemTime,
        cap: u64,
    ) -> Result<(), String> {
        let mut files = Vec::new();
        for entry in fs::read_dir(logs).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("clash-nyanpasu_")
                || !name.ends_with(".log")
                || !entry.file_type().map_err(|e| e.to_string())?.is_file()
            {
                continue;
            }
            let metadata = entry.metadata().map_err(|e| e.to_string())?;
            let modified = metadata.modified().map_err(|e| e.to_string())?;
            if now.duration_since(modified).unwrap_or_default() <= LOG_AGE {
                files.push((modified, entry.path(), metadata.len()));
            }
        }
        files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        let total = files.iter().map(|(_, _, len)| len).sum::<u64>().min(cap);
        let mut read = 0_u64;
        let mut written = 0_u64;
        let mut truncated = files.iter().map(|(_, _, len)| len).sum::<u64>() > cap;
        for (_, path, size) in files {
            if read == total {
                break;
            }
            let amount = size.min(total - read);
            let mut file = File::open(&path).map_err(|e| e.to_string())?;
            let skip = size - amount;
            if skip > 0 {
                file.seek(SeekFrom::Start(skip - 1))
                    .map_err(|e| e.to_string())?;
            }
            let mut reader = BufReader::new(file.take(amount + u64::from(skip > 0)));
            let mut line = Vec::new();
            if skip > 0 {
                reader
                    .read_until(b'\n', &mut line)
                    .map_err(|e| e.to_string())?;
                read += line.len().saturating_sub(1) as u64;
            }
            let mut output = std::collections::VecDeque::new();
            let mut output_bytes = 0_u64;

            loop {
                line.clear();
                let count = reader
                    .read_until(b'\n', &mut line)
                    .map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                read += count as u64;
                let text = String::from_utf8_lossy(&line);
                let redacted = redact_text(&text, self.home.as_deref());
                output_bytes += redacted.len() as u64;
                output.push_back(redacted.into_owned());
                while written + output_bytes > cap {
                    output_bytes -= output
                        .pop_front()
                        .expect("the output exceeds its cap")
                        .len() as u64;
                    truncated = true;
                }
                fraction(if total == 0 {
                    1.
                } else {
                    read as f32 / total as f32
                });
            }
            self.writer
                .start_file(
                    format!("logs/app/{}", path.file_name().unwrap().to_string_lossy()),
                    SimpleFileOptions::default(),
                )
                .map_err(|e| e.to_string())?;
            for line in output {
                self.writer
                    .write_all(line.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            written += output_bytes;
        }
        if truncated {
            report.notes.push(format!(
                "App logs truncated to the newest content within {cap} uncompressed bytes."
            ));
        }
        Ok(())
    }
    pub fn finish(mut self, report: &str) -> Result<PathBuf, String> {
        self.write_entry("report.txt", report.as_bytes())?;
        self.writer
            .finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut index = 0;
        let mut temporary = self.temporary;
        let path = loop {
            let suffix = if index == 0 {
                String::new()
            } else {
                format!("-{index}")
            };
            let path = self.directory.join(format!("{PREFIX}{stamp}{suffix}.zip"));
            match temporary.persist_noclobber(&path) {
                Ok(()) => break path,
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                    temporary = error.path;
                    index += 1;
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        retain_newest(&self.directory).map_err(|e| e.to_string())?;
        Ok(path)
    }
}
fn retain_newest(directory: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(PREFIX) && name.ends_with(".zip") && entry.file_type()?.is_file() {
            files.push((entry.metadata()?.modified()?, entry.path()));
        }
    }
    files.sort_by(|a, b| b.cmp(a));
    for (_, path) in files.into_iter().skip(5) {
        match fs::remove_file(path) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_logs_filters_names_and_retains_five_archives() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("clash-nyanpasu_today.log"),
            "你好 password=abc\nhttps://u:p@host?x=abc\n/home/test/file\n",
        )
        .unwrap();
        fs::write(dir.path().join("other.log"), "excluded").unwrap();
        for _ in 0..7 {
            let mut archive = Archive::new(dir.path(), Some("/home/test".into())).unwrap();
            archive
                .app_logs(dir.path(), &mut Report::default(), &mut |_| {})
                .unwrap();
            let path = archive.finish("report").unwrap();
            let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
            let mut text = String::new();
            zip.by_name("logs/app/clash-nyanpasu_today.log")
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            assert!(!text.contains("abc"));
            assert!(text.contains("~/file"));
            assert!(text.contains("你好"));
            assert!(zip.by_name("logs/app/other.log").is_err());
            assert_eq!(zip.len(), 2);
        }
        assert_eq!(
            fs::read_dir(dir.path().join("diagnostics"))
                .unwrap()
                .count(),
            5
        );
    }
    #[test]
    fn cap_keeps_tail_and_records_truncation_and_age() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clash-nyanpasu_test.log");
        fs::write(&path, "old line\nnew line\n").unwrap();
        let mut archive = Archive::new(dir.path(), None).unwrap();
        let mut report = Report::default();
        archive
            .app_logs_at(dir.path(), &mut report, &mut |_| {}, SystemTime::now(), 10)
            .unwrap();
        assert_eq!(report.notes.len(), 1);
        let path = archive.finish(&report.text()).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_index(0).unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "new line\n");
        let mut archive = Archive::new(dir.path(), None).unwrap();
        archive
            .app_logs_at(
                dir.path(),
                &mut Report::default(),
                &mut |_| {},
                SystemTime::now() + LOG_AGE + Duration::from_secs(1),
                LOG_CAP,
            )
            .unwrap();
        let path = archive.finish("").unwrap();
        assert_eq!(
            zip::ZipArchive::new(File::open(path).unwrap())
                .unwrap()
                .len(),
            1
        );
    }
}
