#[cfg(target_os = "macos")]
mod macos {
    use std::{os::unix::process::ExitStatusExt, path::PathBuf};

    /// use runas to run the command with bash and pipe the output to the tmp output file
    pub fn sudo<M: AsRef<str>, T: AsRef<str>>(bin: M, args: &[T]) -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let script = dir.path().join("script.sh");
        let out = dir.path().join("output.txt");
        if !out.exists() {
            std::fs::write(&out, String::new())?;
        }
        let bin = PathBuf::from(bin.as_ref());
        let parent = bin.parent();
        let mut script_content = String::with_capacity(1024);
        if let Some(parent) = parent {
            script_content.push_str("cd ");
            script_content.push('"');
            script_content.push_str(parent.to_string_lossy().as_ref());
            script_content.push('"');
            script_content.push_str(" && ./");
        }
        script_content.push_str(bin.file_name().unwrap().to_string_lossy().as_ref());
        script_content.push(' ');
        script_content.push_str(
            args.iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(" ")
                .as_ref(),
        );
        tracing::debug!("prepare script: {}", script_content);
        std::fs::write(&script, script_content)?;
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .args([&format!(
                r#"do shell script "bash {} &> {}" with administrator privileges"#,
                script.to_string_lossy(),
                out.to_string_lossy()
            )])
            .status();
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => {
                // read the output file
                let output = std::fs::read_to_string(out)
                    .unwrap_or_else(|e| format!("failed to read output file: {}", e));
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "exit code: {:?}, signal: {:?}, output: {}",
                        status.code(),
                        status.signal(),
                        output
                    ),
                ))
            }
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )),
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::sudo;
