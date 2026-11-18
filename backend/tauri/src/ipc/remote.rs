//! The desktop-only `remote` commands. The shared ones are in `nyanpasu_rpc::commands::remote`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc::error::Result;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt as _;

use crate::adapters::certificate_viewer;

/// Shows the local root clients trust in the platform's read-only certificate viewer. It opens a
/// window on this computer, so only the desktop offers it.
#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn open_remote_access_certificate(
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
) -> Result<()> {
    let certificate = client
        .remote_access_authority_der()
        .await?
        .ok_or_else(|| "no certificate authority has been made yet".to_owned())?;
    certificate_viewer::show(certificate, paths.cache_dir().as_std_path())
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Saves the local root clients trust as a PEM `.crt` file the user chooses, to import on other
/// devices. Returns whether a file was written; `false` when the user cancelled.
#[nyanpasu_rpc::rpc(capabilities(dialog), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn export_remote_access_authority(
    app_handle: AppHandle,
    client: State<'_, NyanpasuClient>,
) -> Result<bool> {
    let certificate = client
        .remote_access_authority_der()
        .await?
        .ok_or_else(|| "no certificate authority has been made yet".to_owned())?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app_handle
        .dialog()
        .file()
        .add_filter("Certificate", &["crt", "pem", "cer"])
        .set_file_name("clash-nyanpasu-local-ca.crt")
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver.await.map_err(|error| error.to_string())? else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|error| error.to_string())?;
    std::fs::write(path, pem(&certificate))?;
    Ok(true)
}

/// The PEM form of a DER certificate, which every trust store imports.
fn pem(certificate_der: &[u8]) -> String {
    use base64::Engine as _;

    let encoded = base64::engine::general_purpose::STANDARD.encode(certificate_der);
    let mut text = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        text.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        text.push('\n');
    }
    text.push_str("-----END CERTIFICATE-----\n");
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_certificate_is_written_as_pem_lines_of_64_characters() {
        let text = super::pem(&[0u8; 100]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.first(), Some(&"-----BEGIN CERTIFICATE-----"));
        assert_eq!(lines.last(), Some(&"-----END CERTIFICATE-----"));
        assert_eq!(lines[1].len(), 64);
        assert!(lines[2].len() <= 64);
        assert!(text.ends_with('\n'));
    }
}
