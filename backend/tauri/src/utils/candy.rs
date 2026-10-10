use anyhow::Result;
use chrono::Local;
use glob::glob;
use std::{
    io::{Seek, Write},
    path::Path,
};
use zip::{ZipWriter, write::SimpleFileOptions};

pub fn collect_logs(target_path: &Path, paths: &nyanpasu_paths::PathResolver) -> Result<()> {
    let file = std::fs::File::create(target_path)?;
    collect_logs_to(file, paths)?;
    Ok(())
}

pub fn collect_logs_tempfile(
    paths: &nyanpasu_paths::PathResolver,
) -> Result<tempfile::NamedTempFile> {
    let file = tempfile::NamedTempFile::new()?;
    collect_logs_to(file.reopen()?, paths)?;
    Ok(file)
}

fn collect_logs_to<W: Write + Seek>(writer: W, paths: &nyanpasu_paths::PathResolver) -> Result<W> {
    let logs_dir = paths.app_logs_dir();
    let now = Local::now().format("%Y-%m-%d");
    let globstr = format!("{}/clash-nyanpasu_{}_*.log", logs_dir, now);
    let mut paths = Vec::new();
    for entry in glob(&globstr)? {
        {
            let path = entry?;
            paths.push(path)
        }
    }
    let mut zip = ZipWriter::new(writer);
    for path in paths {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        zip.start_file(file_name, SimpleFileOptions::default())?;
        let mut file = std::fs::File::open(path)?;
        std::io::copy(&mut file, &mut zip)?;
    }
    Ok(zip.finish()?)
}
