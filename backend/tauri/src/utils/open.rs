use std::ffi::OsStr;

pub fn that<T: AsRef<OsStr>>(path: T) -> std::io::Result<()> {
    // A dirty workaround for AppImage
    if std::env::var("APPIMAGE").is_ok() {
        std::process::Command::new("xdg-open")
            .arg(path)
            .env_remove("LD_LIBRARY_PATH")
            .status()?;
        Ok(())
    } else {
        open::that(path)
    }
}

pub fn with<T: AsRef<OsStr>>(path: T, program: &str) -> std::io::Result<()> {
    // A dirty workaround for AppImage
    if std::env::var("APPIMAGE").is_ok() {
        std::process::Command::new(program)
            .arg(path)
            .env_remove("LD_LIBRARY_PATH")
            .status()?;
        Ok(())
    } else {
        open::with(path, program)
    }
}
