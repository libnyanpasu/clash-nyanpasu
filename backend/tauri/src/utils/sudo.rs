#[cfg(target_os = "macos")]
mod macos {
    pub fn sudo<T: AsRef<str>>(bin: T, args: &[T]) -> std::io::Result<()> {
        use std::process::Command;
        let mut cmd = Command::new("osascript");
        let args = args
            .iter()
            .map(|s| {
                let s: &str = s.as_ref();
                s.replace(" ", "\\\\ ")
            })
            .collect::<Vec<_>>();
        cmd.args([
            "-e",
            &format!(
                "do shell script \"{} {}\" with administrator privileges",
                bin.as_ref(),
                args.join(" ")
            ),
        ]);

        let output = cmd.output()?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = std::str::from_utf8(&output.stderr).unwrap_or("");
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("exit code: {}, err: {}", output.status.code(), stderr),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::sudo;
