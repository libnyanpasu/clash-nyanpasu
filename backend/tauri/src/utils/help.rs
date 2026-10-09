use anyhow::{Context, Result, bail};
use display_info::DisplayInfo;
use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image, ImageRef},
};
use image::{ColorType, ImageEncoder, ImageReader, codecs::png::PngEncoder};
use nanoid::nanoid;
use std::{
    io::{BufWriter, Cursor},
    path::Path,
};
use tauri::{AppHandle, Manager};
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

type Opener = fn(&Path) -> std::io::Result<()>;

/// How [`open_file`] tries to show a file, in order.
const FILE_OPENERS: &[(&str, Opener)] = &[
    ("VS Code", open_with_vscode),
    ("default app", open_with_default_app),
    #[cfg(windows)]
    ("Notepad", open_with_notepad),
    ("file manager", reveal_in_file_manager),
];

/// Opens a file for the user: VS Code when installed, otherwise the default
/// app, then (Windows) Notepad, then the file manager with the file selected.
///
/// Each attempt runs under [`nyanpasu_panics::catch_recoverable`], so a
/// launcher that panics fails over to the next one instead of ending the
/// process. Errors only when every attempt fails.
pub fn open_file(path: &Path) -> Result<()> {
    let mut failures = Vec::new();
    for (name, open) in FILE_OPENERS {
        let failure = match nyanpasu_panics::catch_recoverable(|| open(path)) {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(err)) => err.to_string(),
            Err(panic) => panic.to_string(),
        };
        warn!(path = %path.display(), opener = name, "failed to open file: {failure}");
        failures.push(format!("{name}: {failure}"));
    }
    bail!(
        "failed to open \"{}\" ({})",
        path.display(),
        failures.join("; ")
    )
}

fn open_with_vscode(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let code = "Visual Studio Code";
    #[cfg(windows)]
    let code = "code.cmd";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let code = "code";

    let code_path = which::which(code).map_err(std::io::Error::other)?;
    log::debug!(target: "app", "find VScode `{}`", code_path.display());
    #[cfg(not(windows))]
    {
        crate::utils::open::with(path, code)
    }
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        let mut buf = OsString::with_capacity(path.as_os_str().len() + 2);
        buf.push("\"");
        buf.push(path.as_os_str());
        buf.push("\"");

        open::with_detached(buf, code)
    }
}

fn open_with_default_app(path: &Path) -> std::io::Result<()> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(std::io::Error::other)
}

#[cfg(windows)]
fn open_with_notepad(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("notepad.exe")
        .arg(path)
        .spawn()
        .map(drop)
}

fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(std::io::Error::other)
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
