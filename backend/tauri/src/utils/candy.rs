use super::{config::NyanpasuReqwestProxyExt, dirs::app_logs_dir};
use anyhow::Result;
use chrono::Local;
use glob::glob;
use std::{path::Path, time::Duration};
use url::Url;
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
    path: &'a str,
) -> anyhow::Result<Vec<(&'a str, f64)>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0",
        )
        .build()?;
    // 預熱一下，丟棄第一次的結果
    let requests = mirrors.iter().map(|&mirror| {
        let client = &client;
        let mut url = Url::parse(mirror).unwrap();
        url.set_path(path);
        async move { tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await }
    });
    let _ = futures::future::join_all(requests).await; // 忽略第一次的結果
    let requests = mirrors.iter().map(|&mirror| {
        let client = &client;
        async move {
            let start = tokio::time::Instant::now();
            let mut url = Url::parse(mirror).unwrap();
            url.set_path(path);
            let result = tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await;
            match result {
                Ok(Ok(response)) if response.status().is_success() => {
                    let content_length = response.content_length().unwrap_or(0) as f64;
                    let elapsed = start.elapsed().as_secs_f64();
                    let speed = content_length / elapsed;
                    Some((mirror, speed))
                }
                _ => Some((mirror, 0.0)), // 超时
            }
        }
    });
    let results = futures::future::join_all(requests).await;
    let mut results = results.into_iter().flatten().collect::<Vec<_>>();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    Ok(results)
}

mod test {
    #[allow(unused_imports)]
    use super::*;

    #[tokio::test]
    async fn test_mirror_speed_test() {
        let mirrors = &[
            "https://github.com/",
            "https://gh-proxy.com/",
            "https://ghproxy.org/",
            "https://mirror.ghproxy.com/",
            "https://gh.idayer.com/",
        ];
        let results = mirror_speed_test(mirrors, "https://gist.githubusercontent.com/khaykov/a6105154becce4c0530da38e723c2330/raw/41ab415ac41c93a198f7da5b47d604956157c5c3/gistfile1.txt").await.unwrap();
        println!("{:?}", results);
        assert_eq!(results.len(), 5);
    }
}
