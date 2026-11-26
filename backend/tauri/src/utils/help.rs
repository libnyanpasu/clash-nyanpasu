use anyhow::{Context, Result, bail};
use display_info::DisplayInfo;
use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image, ImageRef},
};
use image::{ColorType, ImageEncoder, ImageReader, codecs::png::PngEncoder};
use nanoid::nanoid;
use nyanpasu_config::application::ProfileFileOpener;
use std::{
    io::{BufWriter, Cursor},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use tokio::process::Command;
use tracing::{debug, warn};
use tracing_attributes::instrument;

const ALPHABET: [char; 62] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i',
    'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B',
    'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U',
    'V', 'W', 'X', 'Y', 'Z',
];

/// generate the uid
pub fn get_uid(prefix: &str) -> String {
    let id = nanoid!(11, &ALPHABET);
    format!("{prefix}{id}")
}

/// Open with the selected program, without falling back to another editor.
pub fn open_profile_file(path: &Path, opener: &ProfileFileOpener) -> Result<()> {
    if matches!(opener, ProfileFileOpener::SystemDefault) {
        nyanpasu_panics::catch_recoverable(|| tauri_plugin_opener::open_path(path, None::<&str>))??;
        return Ok(());
    }

    let executable = match opener {
        ProfileFileOpener::Custom { path } => {
            validate_profile_file_opener(opener)?;
            path.clone()
        }
        _ => find_profile_editor(opener)?,
    };
    let mut command = Command::new(&executable);
    command.arg(path);
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        command.env_remove("LD_LIBRARY_PATH");
    }
    command
        .spawn()
        .with_context(|| format!("failed to launch editor {}", executable.display()))?;
    Ok(())
}

/// A custom editor must be an existing absolute executable path, never a shell command.
pub fn validate_profile_file_opener(opener: &ProfileFileOpener) -> Result<()> {
    if let ProfileFileOpener::Custom { path } = opener
        && !is_executable(path)
    {
        bail!("not an executable file: {}", path.display());
    }
    #[cfg(not(windows))]
    if matches!(opener, ProfileFileOpener::NotepadPlusPlus) {
        bail!("Notepad++ is only available on Windows");
    }
    Ok(())
}

fn is_executable(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
}

fn find_profile_editor(opener: &ProfileFileOpener) -> Result<PathBuf> {
    let (label, name) = match opener {
        ProfileFileOpener::VsCode => ("VS Code", if cfg!(windows) { "Code.exe" } else { "code" }),
        ProfileFileOpener::Zed => ("Zed", if cfg!(windows) { "zed.exe" } else { "zed" }),
        ProfileFileOpener::NotepadPlusPlus if cfg!(windows) => ("Notepad++", "notepad++.exe"),
        _ => bail!("editor is not available on this platform"),
    };
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        let relative = match opener {
            ProfileFileOpener::VsCode => "Microsoft VS Code/Code.exe",
            ProfileFileOpener::Zed => "Zed/zed.exe",
            ProfileFileOpener::NotepadPlusPlus => "Notepad++/notepad++.exe",
            _ => unreachable!("only supported presets reach here"),
        };
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("Programs").join(relative));
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(programs) = std::env::var_os(variable) {
                candidates.push(PathBuf::from(programs).join(relative));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let relative = match opener {
            ProfileFileOpener::VsCode => "Visual Studio Code.app/Contents/Resources/app/bin/code",
            ProfileFileOpener::Zed => "Zed.app/Contents/MacOS/zed",
            _ => unreachable!("only supported presets reach here"),
        };
        candidates.push(PathBuf::from("/Applications").join(relative));
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join("Applications").join(relative));
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".local/bin").join(name));
        }
        for directory in ["/usr/bin", "/usr/local/bin"] {
            candidates.push(Path::new(directory).join(name));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|directory| directory.join(name)));
    }
    candidates
        .into_iter()
        .find(|candidate| is_executable(candidate))
        .with_context(|| {
            format!("{label} was not found; install it or choose another editor in Settings")
        })
}

/// Reveal a file in its parent directory without opening it.
pub fn reveal_in_file_manager(path: &Path) -> Result<()> {
    nyanpasu_panics::catch_recoverable(|| tauri_plugin_opener::reveal_item_in_dir(path))??;
    Ok(())
}

/// Resolve the UI language from the OS locale, as the canonical i18n key.
///
/// The key names a `rust_i18n` bundle in `backend/tauri/locales` and a paraglide
/// locale in the frontend; both use the same lowercase spelling.
pub fn detect_system_i18n_key() -> &'static str {
    nyanpasu_config::application::default_i18n_language().as_str()
}

pub fn resize_tray_image(img: &[u8], scale_factor: f64) -> Result<Vec<u8>> {
    let img = ImageReader::new(Cursor::new(img))
        .with_guessed_format()?
        .decode()?;
    let width = img.width();
    let height = img.height();
    let src_pixels = img.into_rgba8().into_raw();
    let src_image = ImageRef::new(width, height, &src_pixels, PixelType::U8x4)
        .context("failed to parse image")?;

    // Create container for data of destination image
    let size = (32_f64 * scale_factor).round() as u32; // 32px is the base tray size as the dpi is 96
    let dst_width = size;
    let dst_height = size;
    let mut dst_image = Image::new(dst_width, dst_height, src_image.pixel_type());

    // Create Resizer instance and resize source image
    // into buffer of destination image
    let mut resizer = Resizer::new();
    let resizer_options = ResizeOptions {
        algorithm: ResizeAlg::Convolution(FilterType::Lanczos3),
        ..Default::default()
    };
    resizer
        .resize(&src_image, &mut dst_image, &resizer_options)
        .context("failed to resize image")?;

    // Extract raw pixel data from the destination image
    let dst_image_data = dst_image.buffer().to_vec();

    // Write destination image as PNG-file
    let mut result_buf = BufWriter::new(Vec::new());
    PngEncoder::new(&mut result_buf).write_image(
        &dst_image_data,
        dst_width,
        dst_height,
        ColorType::Rgba8.into(),
    )?;
    Ok(result_buf.into_inner()?)
}

#[instrument]
pub fn get_max_scale_factor() -> f64 {
    match DisplayInfo::all() {
        Ok(displays) => {
            let mut scale_factor = 0.0;
            debug!("displays: {:?}", displays);
            for display in displays {
                if display.scale_factor > scale_factor {
                    scale_factor = display.scale_factor;
                }
            }
            scale_factor as f64
        }
        Err(err) => {
            warn!("failed to get display info: {:?}", err);
            1.0_f64
        }
    }
}

#[instrument(skip(app_handle))]
pub fn quit_application(app_handle: &AppHandle) {
    app_handle.exit(0);
}

/// Exits through the exit boundary, which starts the relauncher once every
/// owner has shut down.
#[instrument(skip(app_handle))]
pub fn restart_application(app_handle: &AppHandle) {
    app_handle
        .state::<super::exit::ExitBoundary>()
        .request_restart();
    app_handle.exit(0);
}

#[macro_export]
macro_rules! error {
    ($result: expr) => {
        log::error!(target: "app", "{:?}", $result);
    };
}

#[macro_export]
macro_rules! log_err {
    ($result: expr) => {
        if let Err(err) = $result {
            log::error!(target: "app", "{:#?}", err);
        }
    };

    ($result: expr, $label: expr) => {
        if let Err(err) = $result {
            log::error!(target: "app", "{}: {:#?}", $label, err);
        }
    };
}

#[macro_export]
macro_rules! dialog_err {
    ($result: expr) => {
        if let Err(err) = $result {
            $crate::utils::dialog::error_dialog(format!("{:?}", err));
        }
    };

    ($result: expr, $err_str: expr) => {
        if let Err(_) = $result {
            $crate::utils::dialog::error_dialog($err_str.into());
        }
    };
}

#[macro_export]
macro_rules! trace_err {
    ($result: expr, $err_str: expr) => {
        if let Err(err) = $result {
            log::trace!(target: "app", "{}, err {:?}", $err_str, err);
        }
    }
}

#[cfg(test)]
mod profile_opener_tests {
    use super::*;

    #[test]
    fn unavailable_custom_editor_is_an_error_not_a_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("profile.yaml");
        std::fs::write(&profile, "proxies: []").unwrap();
        let opener = ProfileFileOpener::Custom {
            path: directory.path().join("missing.exe"),
        };
        let error = open_profile_file(&profile, &opener).unwrap_err();
        assert!(error.to_string().contains("not an executable file"));
    }

    #[test]
    fn custom_editor_must_be_an_absolute_executable_file() {
        let directory = tempfile::tempdir().unwrap();
        for path in [
            PathBuf::from("relative-editor"),
            directory.path().to_owned(),
            directory.path().join("missing.exe"),
        ] {
            assert!(validate_profile_file_opener(&ProfileFileOpener::Custom { path }).is_err());
        }
        let executable = std::env::current_exe().unwrap();
        assert!(
            validate_profile_file_opener(&ProfileFileOpener::Custom { path: executable }).is_ok()
        );
    }
}
