use crate::config::nyanpasu::ExternalControllerPortStrategy;
use anyhow::{anyhow, bail, Context, Result};
use display_info::DisplayInfo;
use fast_image_resize::{
    images::{Image, ImageRef},
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, ImageReader};
use nanoid::nanoid;
use serde::{de::DeserializeOwned, Serialize};
use serde_yaml::{Mapping, Value};
use std::{
    fs,
    io::{BufWriter, Cursor},
    path::PathBuf,
    str::FromStr,
};
use tauri::{process::current_binary, AppHandle, Manager};
use tauri_plugin_shell::ShellExt;
use tracing::{debug, warn};
use tracing_attributes::instrument;

use crate::trace_err;

/// read data from yaml as struct T
pub fn read_yaml<T: DeserializeOwned>(path: &PathBuf) -> Result<T> {
    if !path.exists() {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = fs::read_to_string(path)
        .with_context(|| format!("failed to read the file \"{}\"", path.display()))?;

    serde_yaml::from_str::<T>(&yaml_str).with_context(|| {
        format!(
            "failed to read the file with yaml format \"{}\"",
            path.display()
        )
    })
}

/// read mapping from yaml fix #165
pub fn read_merge_mapping(path: &PathBuf) -> Result<Mapping> {
    let mut val: Value = read_yaml(path)?;
    val.apply_merge()
        .with_context(|| format!("failed to apply merge \"{}\"", path.display()))?;

    Ok(val
        .as_mapping()
        .ok_or(anyhow!(
            "failed to transform to yaml mapping \"{}\"",
            path.display()
        ))?
        .to_owned())
}

/// save the data to the file
/// can set `prefix` string to add some comments
pub fn save_yaml<T: Serialize>(path: &PathBuf, data: &T, prefix: Option<&str>) -> Result<()> {
    let data_str = serde_yaml::to_string(data)?;

    let yaml_str = match prefix {
        Some(prefix) => format!("{prefix}\n\n{data_str}"),
        None => data_str,
    };

    let path_str = path.as_os_str().to_string_lossy().to_string();
    fs::write(path, yaml_str.as_bytes())
        .with_context(|| format!("failed to save file \"{path_str}\""))
}

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

/// parse the string
/// xxx=123123; => 123123
pub fn parse_str<T: FromStr>(target: &str, key: &str) -> Option<T> {
    target.split(';').map(str::trim).find_map(|s| {
        let mut parts = s.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some(k), Some(v)) if k == key => v.parse::<T>().ok(),
            _ => None,
        }
    })
}

/// open file
/// use vscode by default
pub fn open_file(app: tauri::AppHandle, path: PathBuf) -> Result<()> {
    #[cfg(target_os = "macos")]
    let code = "Visual Studio Code";
    #[cfg(windows)]
    let code = "code.cmd";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let code = "code";

    let shell = app.shell();

    trace_err!(
        match which::which(code) {
            Ok(code_path) => {
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
            Err(err) => {
                log::error!(target: "app", "Can't find VScode `{err:?}`");
                // default open
                shell
                    .open(path.to_string_lossy().to_string(), None)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
            }
        },
        "Can't open file"
    );

    Ok(())
}

pub fn get_system_locale() -> String {
    tauri_plugin_os::locale().unwrap_or("en-US".to_string())
}

pub fn mapping_to_i18n_key(locale_key: &str) -> &'static str {
    if locale_key.starts_with("zh-") {
        "zh"
    } else {
        "en"
    }
}

pub fn get_clash_external_port(
    strategy: &ExternalControllerPortStrategy,
    port: u16,
) -> anyhow::Result<u16> {
    match strategy {
        ExternalControllerPortStrategy::Fixed => {
            if !port_scanner::local_port_available(port) {
                bail!("Port {} is not available", port);
            }
        }
        ExternalControllerPortStrategy::Random | ExternalControllerPortStrategy::AllowFallback => {
            if ExternalControllerPortStrategy::AllowFallback == *strategy
                && port_scanner::local_port_available(port)
            {
                return Ok(port);
            }
            let new_port = port_scanner::request_open_port()
                .ok_or_else(|| anyhow!("Can't find an open port"))?;
            return Ok(new_port);
        }
    }
    Ok(port)
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
pub fn cleanup_processes(app_handle: &AppHandle) {
    let _ = super::resolve::save_window_state(app_handle, true);
    super::resolve::resolve_reset();
    let _ = nyanpasu_utils::runtime::block_on(async move {
        crate::core::CoreManager::global().stop_core().await
    });
}

#[instrument(skip(app_handle))]
pub fn quit_application(app_handle: &AppHandle) {
    app_handle.exit(0);
}

#[instrument(skip(app_handle))]
pub fn restart_application(app_handle: &AppHandle) {
    cleanup_processes(app_handle);
    let env = app_handle.env();
    let path = current_binary(&env).unwrap();
    let arg = std::env::args().collect::<Vec<String>>();
    let mut args = vec!["launch".to_string(), "--".to_string()];
    // filter out the first arg
    if arg.len() > 1 {
        args.extend(arg.iter().skip(1).cloned());
    }
    tracing::info!("restart app: {:#?} with args: {:#?}", path, args);
    std::process::Command::new(path)
        .args(args)
        .spawn()
        .expect("application failed to start");
    app_handle.exit(0);
    std::process::exit(0);
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

#[test]
fn test_parse_value() {
    let test_1 = "upload=111; download=2222; total=3333; expire=444";
    let test_2 = "attachment; filename=Clash.yaml";

    assert_eq!(parse_str::<usize>(test_1, "upload").unwrap(), 111);
    assert_eq!(parse_str::<usize>(test_1, "download").unwrap(), 2222);
    assert_eq!(parse_str::<usize>(test_1, "total").unwrap(), 3333);
    assert_eq!(parse_str::<usize>(test_1, "expire").unwrap(), 444);
    assert_eq!(
        parse_str::<String>(test_2, "filename").unwrap(),
        format!("Clash.yaml")
    );

    assert_eq!(parse_str::<usize>(test_1, "aaa"), None);
    assert_eq!(parse_str::<usize>(test_1, "upload1"), None);
    assert_eq!(parse_str::<usize>(test_1, "expire1"), None);
    assert_eq!(parse_str::<usize>(test_2, "attachment"), None);
}
