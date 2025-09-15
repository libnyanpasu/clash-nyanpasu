use crate::{AutoLaunch, Result};
use std::{fs, io::Write, path::PathBuf, env};

/// Linux implement
impl AutoLaunch {
    /// Create a new AutoLaunch instance
    /// - `app_name`: application name
    /// - `app_path`: application path
    /// - `args`: startup args passed to the binary
    ///
    /// ## Notes
    ///
    /// The parameters of `AutoLaunch::new` are different on each platform.
    pub fn new(app_name: &str, app_path: &str, args: &[impl AsRef<str>]) -> AutoLaunch {
        AutoLaunch {
            app_name: app_name.into(),
            app_path: app_path.into(),
            args: args.iter().map(|s| s.as_ref().to_string()).collect(),
        }
    }

    /// Enable the AutoLaunch setting
    ///
    /// ## Errors
    ///
    /// - failed to create dir `~/.config/autostart`
    /// - failed to create file `~/.config/autostart/{app_name}.desktop`
    /// - failed to write bytes to the file
    pub fn enable(&self) -> Result<()> {
        // Detect desktop environment
        let desktop_env = env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| env::var("DESKTOP_SESSION"))
            .unwrap_or_else(|_| "unknown".to_string())
            .to_lowercase();
        
        // Build basic desktop file content
        let mut data = format!(
            "[Desktop Entry]\n\
            Type=Application\n\
            Version=1.0\n\
            Name={}\n\
            Comment={}startup script\n\
            Exec={} {}\n\
            StartupNotify=false\n\
            Terminal=false",
            self.app_name,
            self.app_name,
            self.app_path,
            self.args.join(" ")
        );
        
        // Add special configuration for KDE environment
        if desktop_env.contains("kde") || desktop_env.contains("plasma") {
            data.push_str("\nX-KDE-autostart-after=panel");
        }

        let dir = get_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir).or_else(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.get_file())?;
        file.write_all(data.as_bytes())?;
        Ok(())
    }

    /// Disable the AutoLaunch setting
    ///
    /// ## Errors
    ///
    /// - failed to remove file `~/.config/autostart/{app_name}.desktop`
    pub fn disable(&self) -> Result<()> {
        let file = self.get_file();
        if file.exists() {
            fs::remove_file(file)?;
        }
        Ok(())
    }

    /// Check whether the AutoLaunch setting is enabled
    pub fn is_enabled(&self) -> Result<bool> {
        Ok(self.get_file().exists())
    }

    /// Get the desktop entry file path
    fn get_file(&self) -> PathBuf {
        get_dir().join(format!("{}.desktop", self.app_name))
    }
}

/// Get the autostart dir
fn get_dir() -> PathBuf {
    dirs::config_dir().unwrap().join("autostart")
}
