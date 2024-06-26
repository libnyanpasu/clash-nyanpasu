use futures::StreamExt;
/// Downloader is a utility to download file with parallel requests and progress bar.
/// TODO: use &str instead of String to avoid unnecessary allocation
use num_cpus;
use reqwest::Client;
use std::{sync::Arc, time};
use tempfile::tempfile;
use thiserror::Error;
use tokio::{
    fs::File,
    io::AsyncWriteExt,
    sync::{
        mpsc::{self, Sender},
        RwLock, Semaphore,
    },
    time::sleep,
};

pub struct Downloader {
    client: Client,
    url: String,
    file: File,
    total_size: u64,
    semaphore: Arc<Semaphore>,
    parts: Vec<Arc<RwLock<Thread>>>,
}

enum ThreadEvent {
    DecreaseSemaphore(DecreaseSemaphoreReason),
    Finish,
}

enum DecreaseSemaphoreReason {
    Reason(String),
    Cause(anyhow::Error),
}

enum ThreadState {
    Idle,
    Downloading,
    Finished,
}

struct Thread {
    client: Client,
    sender: Sender<ThreadEvent>,
    semaphore: Arc<Semaphore>,
    file: File,
    url: String,
    pub state: ThreadState,
    pub start: usize,
    pub end: usize,
    pub downloaded: usize,
    pub speed: f64,
}

#[derive(Error, Debug)]
pub enum DownloaderError {
    #[error("Failed to perform a request")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Failed to download file")]
    DownloadFailed(#[from] anyhow::Error),
    #[error("Failed to write file")]
    WriteFailed(#[from] std::io::Error),
    #[error("Failed to confirm file size")]
    ConfirmSizeFailed,

    #[error("Other error: {0}")]
    Other(String),
}

impl Downloader {
    pub fn new(url: String, file: File) -> Self {
        let client = reqwest::Client::new();
        let nums = num_cpus::get();
        Downloader {
            client,
            url,
            file,
            total_size: 0,
            parts: Vec::with_capacity(nums),
            semaphore: Arc::new(Semaphore::new(num_cpus::get())),
        }
    }

    pub fn set_client(&mut self, client: Client) {
        self.client = client;
    }

    async fn confirm_file_size(&mut self) -> Result<(), DownloaderError> {
        let response = self
            .client
            .head(&self.url)
            .send()
            .await?
            .error_for_status()?;
        let headers = response.headers();
        let total_size = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if total_size == 0 {
            return Err(DownloaderError::ConfirmSizeFailed);
        }
        self.total_size = total_size;
        self.file.set_len(total_size).await?;
        Ok(())
    }

    pub async fn download(&mut self) -> Result<(), DownloaderError> {
        self.confirm_file_size().await?;
        let counts = self.semaphore.available_permits() as u64;
        let part_size = self.total_size / counts;
        let (tx, mut rx) = mpsc::channel(10);

        for chunk in 0..counts {
            let start = (chunk * part_size) as usize;
            let end = if chunk == counts - 1 {
                self.total_size as usize
            } else {
                ((chunk + 1) * part_size) as usize
            };
            let thread = Arc::new(RwLock::new(Thread::try_new(
                self.client.clone(),
                tx.clone(),
                self.semaphore.clone(),
                start,
                end,
                self.url.clone(),
            )?));
            let thread_clone = thread.clone();
            tokio::spawn(async move {
                let mut thread = thread_clone.write().await;
                thread.start().await;
            });
            self.parts.push(thread);
        }
        // TODO: 根據情況嘗試恢復 semaphore 數目
        let mut downloaded = 0;
        let mut total_permits = counts;
        while let Some(event) = rx.recv().await {
            match event {
                ThreadEvent::Finish => {
                    downloaded += 1;
                    if downloaded == counts {
                        break;
                    }
                }
                ThreadEvent::DecreaseSemaphore(reason) => {
                    total_permits -= 1;
                    // 儅 semaphore 為 0 時，表示無可用下載綫程，説明文件無法下載
                    if total_permits == 0 {
                        match reason {
                            DecreaseSemaphoreReason::Cause(e) => {
                                return Err(DownloaderError::DownloadFailed(e));
                            }
                            DecreaseSemaphoreReason::Reason(e) => {
                                return Err(DownloaderError::Other(e));
                            }
                        }
                    }
                }
            }
        }
        // 合并文件
        Ok(())
    }
}

impl Thread {
    pub fn try_new(
        client: Client,
        sender: Sender<ThreadEvent>,
        semaphore: Arc<Semaphore>,
        start: usize,
        end: usize,
        url: String,
    ) -> std::io::Result<Self> {
        let file = tempfile()?;
        let file = File::from_std(file);
        Ok(Self {
            client,
            sender,
            semaphore,
            state: ThreadState::Idle,
            start,
            end,
            file,
            url,
            downloaded: 0,
            speed: 0.0,
        })
    }

    pub async fn download_chunk(&mut self) -> Result<(), anyhow::Error> {
        let response = self
            .client
            .get(&self.url)
            .header(
                reqwest::header::RANGE,
                format!("bytes={}-{}", self.start, self.end),
            )
            .send()
            .await?
            .error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut tick = time::Instant::now();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            self.speed = chunk.len() as f64 / tick.elapsed().as_secs_f64();
            self.file.write_all(&chunk).await?;
            self.downloaded += chunk.len();
            tick = time::Instant::now();
        }
        Ok(())
    }

    pub async fn start(&mut self) {
        let mut attempts = 0;
        let semaphore = self.semaphore.clone();
        loop {
            let _permit = match semaphore.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    break; // semaphore 已經被釋放
                }
            };
            drop(_permit);
            match self.download_chunk().await {
                Ok(_) => {
                    self.state = ThreadState::Finished;
                    self.sender.send(ThreadEvent::Finish).await.unwrap();
                    break;
                }
                Err(_) if attempts < 3 => {
                    attempts += 1;
                    sleep(time::Duration::from_secs(1)).await;
                }
                Err(e) => {
                    self.sender
                        .send(ThreadEvent::DecreaseSemaphore(
                            DecreaseSemaphoreReason::Cause(e),
                        ))
                        .await
                        .unwrap();
                    self.semaphore.forget_permits(1); // 釋放自身的 semaphore
                    attempts = 0;
                }
            }
        }
    }
}

mod test {
    use super::*;
    use tokio::fs::File as TokioFile;

    #[tokio::test]
    async fn test_downloader() {
        let file = TokioFile::create("test.txt").await.unwrap();
        let tick = time::Instant::now();
        let mut downloader = Downloader::new(
            "http://hkg.download.datapacket.com/100mb.bin".to_string(),
            file,
        );
        downloader.download().await.unwrap();
        println!("Time elapsed: {:?}", tick.elapsed());
    }
}
