use super::{config::NyanpasuReqwestProxyExt, dirs::app_logs_dir};
use anyhow::Result;
use chrono::Local;
use glob::glob;
use std::{path::Path, time::Duration};
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

pub async fn mirror_speed_test<'a>(
    mirrors: &'a [&'a str],
) -> anyhow::Result<Vec<(&'a str, Duration)>> {
    let client = reqwest::Client::new();
    // 預熱一下，丟棄第一次的結果
    let requests = mirrors.iter().map(|&url| {
        let client = &client;
        async move { tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await }
    });
    let _ = futures::future::join_all(requests).await; // 忽略第一次的結果
    let requests = mirrors.iter().map(|&url| {
        let client = &client;
        async move {
            let start = tokio::time::Instant::now();
            let result = tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await;
            match result {
                Ok(Ok(response)) if response.status().is_success() => {
                    let elapsed = start.elapsed();
                    Some((url, elapsed))
                }
                _ => None,
            }
        }
    });
    let results = futures::future::join_all(requests).await;
    let results = results.into_iter().flatten().collect();
    Ok(results)
}

mod test {
    pub use super::*;

    #[tokio::test]
    async fn test_mirror_speed_test() {
        let mirrors = &[
            "https://github.com",
            "https://gh-proxy.com",
            "https://ghproxy.org/",
            "https://mirror.ghproxy.com",
            "https://gh.idayer.com/",
        ];
        let results = mirror_speed_test(mirrors).await.unwrap();
        println!("{:?}", results);
        assert_eq!(results.len(), 5);
    }
}
