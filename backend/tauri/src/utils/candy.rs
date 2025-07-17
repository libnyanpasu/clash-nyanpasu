use super::{config::NyanpasuReqwestProxyExt, dirs::app_logs_dir};
use anyhow::Result;
use chrono::Local;
use glob::glob;
use std::{path::Path, time::Duration};
use url::Url;
use zip::{ZipWriter, write::SimpleFileOptions};

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
        .user_agent(format!("clash-nyanpasu/{app_version}"))
        .build()?;
    Ok(client)
}

pub const INTERNAL_MIRRORS: &[&str] = &[
    "https://github.com/",
    "https://gh-proxy.com/",
    // too many restrictions, not recommended
    // "https://gh.idayer.com/",
];

pub fn parse_gh_url(mirror: &str, path: &str) -> Result<Url, url::ParseError> {
    if mirror.contains("github.com") && !path.starts_with('/') {
        Url::parse(path)
    } else {
        let mut url = Url::parse(mirror)?;
        url.set_path(path);
        Ok(url)
    }
}

#[async_trait::async_trait]
pub trait ReqwestSpeedTestExt {
    async fn mirror_speed_test<'a>(
        &self,
        mirrors: &'a [&'a str],
        path: &'a str,
    ) -> Result<Vec<(&'a str, f64)>>;
}

#[async_trait::async_trait]
impl ReqwestSpeedTestExt for reqwest::Client {
    async fn mirror_speed_test<'a>(
        &self,
        mirrors: &'a [&'a str],
        path: &'a str,
    ) -> Result<Vec<(&'a str, f64)>> {
        let results = futures::future::join_all(mirrors.iter().map(|&mirror| {
            let client = self;
            async move {
                let start = tokio::time::Instant::now();
                // if mirror is github.com, we should use it directly
                let url = parse_gh_url(mirror, path)?;
                tracing::debug!("Testing {}", url.as_str());
                let _ =
                    tokio::time::timeout(Duration::from_secs(3), client.get(url.as_str()).send())
                        .await; // warm up
                let result: Result<reqwest::Response, anyhow::Error> =
                    tokio::time::timeout(Duration::from_secs(3), client.get(url).send())
                        .await
                        .map_err(anyhow::Error::msg)
                        .and_then(|v| v.map_err(anyhow::Error::msg))
                        .and_then(|v| v.error_for_status().map_err(anyhow::Error::msg));
                match result {
                    Ok(response) => {
                        let content_length = response.content_length().unwrap_or(0) as f64;
                        // should read all the response body to get the correct speed
                        match response.bytes().await {
                            Ok(_) => {
                                let elapsed = start.elapsed().as_secs_f64();
                                let speed = content_length / elapsed;
                                Ok((mirror, speed))
                            }
                            Err(e) => {
                                tracing::warn!("test mirror {} failed: {}", mirror, e);
                                Ok((mirror, 0.0))
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("test mirror {} failed: {}", mirror, e);
                        Ok((mirror, 0.0))
                    }
                }
            }
        }))
        .await;
        let collected_result: Result<Vec<_>, anyhow::Error> = results.into_iter().collect();
        let mut results = collected_result?;
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        Ok(results)
    }
}

mod test {
    #[allow(unused_imports)]
    use super::*;

    #[tokio::test]
    #[allow(clippy::needless_return)] // a bug in clippy
    async fn test_mirror_speed_test() {
        let client = reqwest::Client::builder().user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
        ).build().unwrap();
        let results = client
            .mirror_speed_test(
                INTERNAL_MIRRORS,
                "https://raw.githubusercontent.com/simonw/github-large-file-test/master/1.5mb.txt",
            )
            .await
            .unwrap();
        println!("{results:?}");
    }
}
