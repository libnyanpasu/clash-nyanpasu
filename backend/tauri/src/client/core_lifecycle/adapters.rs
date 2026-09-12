use super::ports::{BinaryInstaller, PreparedCoreBinary};
use async_trait::async_trait;

pub struct FsBinaryInstaller;

#[async_trait]
impl BinaryInstaller for FsBinaryInstaller {
    async fn install(&self, artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
        if let Err(error) = tokio::fs::copy(&artifact.source, &artifact.destination).await {
            tracing::warn!(%error, "core copy failed; requesting elevated installation");
            let source = artifact.source.clone();
            let destination = artifact.destination.clone();
            // The blocking task itself retains the staging files if its waiter dies.
            let staging = artifact.staging.clone();
            let status = tokio::task::spawn_blocking(move || {
                let _staging = staging;
                #[cfg(target_os = "windows")]
                {
                    let source = source
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 source"))?;
                    let destination = destination
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 destination"))?;
                    Ok::<_, anyhow::Error>(
                        runas::Command::new("cmd")
                            .args(&[
                                "/C",
                                "copy",
                                "/Y",
                                source,
                                destination.trim_start_matches(r"\\?\"),
                            ])
                            .status()?,
                    )
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Ok::<_, anyhow::Error>(
                        runas::Command::new("cp")
                            .arg("-f")
                            .arg(source)
                            .arg(destination)
                            .status()?,
                    )
                }
            })
            .await??;
            anyhow::ensure!(status.success(), "failed to copy core: {status}");
        }
        Ok(())
    }
}
