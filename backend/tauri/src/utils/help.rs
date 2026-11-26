use anyhow::Result;
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

/// Open a file using its system association.
pub fn open_with_default_app(path: &Path) -> Result<()> {
    nyanpasu_panics::catch_recoverable(|| tauri_plugin_opener::open_path(path, None::<&str>))??;
    Ok(())
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
    use anyhow::Context as _;
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
