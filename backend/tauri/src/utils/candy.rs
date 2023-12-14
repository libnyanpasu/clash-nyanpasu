use super::dirs::app_logs_dir;
use anyhow::Result;
use chrono::Local;
use glob::glob;
use std::path::Path;
use zip::ZipWriter;
pub fn collect_logs(target_path: &Path) -> Result<()> {
    let logs_dir = app_logs_dir()?;
    let now = Local::now().format("%Y-%m-%d");
    let globstr = format!("{}/{}-*.log", logs_dir.to_str().unwrap(), now);
    let mut paths = Vec::new();
    for entry in glob(&globstr)? {
        match entry {
            Ok(path) => paths.push(path),
            Err(e) => return Err(e.into()),
        }
    }
    let file = std::fs::File::create(target_path)?;
    let mut zip = ZipWriter::new(file);
    for path in paths {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        zip.start_file(file_name, Default::default())?;
        let mut file = std::fs::File::open(path)?;
        std::io::copy(&mut file, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}
