use super::{config::NyanpasuReqwestProxyExt, dirs::app_logs_dir};
use anyhow::Result;
use chrono::Local;
use glob::glob;
use std::path::Path;
use zip::{write::SimpleFileOptions, ZipWriter};

pub fn collect_logs(target_path: &Path) -> Result<()> {
    let logs_dir = app_logs_dir()?;
    let now = Local::now().format("%Y-%m-%d");
    let globstr = format!("{}/*.{}.app.log", logs_dir.to_str().unwrap(), now);
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
        zip.start_file(file_name, SimpleFileOptions::default())?;
        let mut file = std::fs::File::open(path)?;
        std::io::copy(&mut file, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

// TODO: 添加自定义 User-Agent 等配置，说白了就是重构一下 prfitem 的那坨代码
pub fn get_reqwest_client() -> Result<reqwest::Client> {
    let builder = reqwest::ClientBuilder::new();
    let app_version = super::dirs::get_app_version();
    let client = builder
        .swift_set_nyanpasu_proxy()
        .user_agent(format!("clash-nyanpasu/{}", app_version))
        .build()?;
    Ok(client)
}
