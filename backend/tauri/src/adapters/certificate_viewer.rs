//! Shows the HTTPS certificate in the platform's own read-only viewer: the Windows certificate
//! dialog, Quick Look on macOS, and `gcr-viewer` (or the desktop's handler for `.cer`) on Linux.
//! None of them imports the certificate into a trust store.

use snafu::Snafu;

/// Why the viewer could not be opened.
#[derive(Debug, Snafu)]
pub enum CertificateViewerError {
    /// Only the viewers that open a file need one.
    #[cfg(not(windows))]
    #[snafu(display("failed to write the certificate for the viewer"))]
    WriteFile { source: std::io::Error },
    #[snafu(display("failed to start the certificate viewer"))]
    Start { source: std::io::Error },
    #[cfg(not(windows))]
    #[snafu(display("no certificate viewer is installed"))]
    NoViewer,
}

/// Opens `certificate_der` without waiting for the viewer to close. `cache_dir` holds the file a
/// viewer that reads one opens; it is replaced on every call.
#[cfg(windows)]
pub fn show(
    certificate_der: Vec<u8>,
    _cache_dir: &std::path::Path,
) -> Result<(), CertificateViewerError> {
    use windows_sys::Win32::Security::Cryptography::{
        CERT_STORE_CERTIFICATE_CONTEXT, CertCreateCertificateContext, CertFreeCertificateContext,
        PKCS_7_ASN_ENCODING, UI::CryptUIDlgViewContext, X509_ASN_ENCODING,
    };

    let length =
        u32::try_from(certificate_der.len()).expect("a certificate is far smaller than 4 GiB");
    // The dialog is modal to its own thread and returns only when the user closes it.
    std::thread::Builder::new()
        .name("certificate-viewer".to_owned())
        .spawn(move || {
            // SAFETY: the pointer and length describe `certificate_der`, which outlives both calls;
            // a null context means the bytes were refused and nothing is shown or freed. The
            // context is freed once, after the dialog that borrowed it returned.
            unsafe {
                let context = CertCreateCertificateContext(
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    certificate_der.as_ptr(),
                    length,
                );
                if context.is_null() {
                    tracing::warn!("the certificate could not be loaded for the viewer");
                    return;
                }
                CryptUIDlgViewContext(
                    CERT_STORE_CERTIFICATE_CONTEXT,
                    context.cast(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                );
                CertFreeCertificateContext(context);
            }
        })
        .map_err(|source| CertificateViewerError::Start { source })?;
    Ok(())
}

#[cfg(not(windows))]
pub fn show(
    certificate_der: Vec<u8>,
    cache_dir: &std::path::Path,
) -> Result<(), CertificateViewerError> {
    use snafu::ResultExt as _;

    std::fs::create_dir_all(cache_dir).context(WriteFileSnafu)?;
    let path = cache_dir.join("remote-access-certificate.cer");
    std::fs::write(&path, certificate_der).context(WriteFileSnafu)?;

    #[cfg(target_os = "macos")]
    let viewers: &[(&str, &[&str])] = &[("qlmanage", &["-p"])];
    #[cfg(not(target_os = "macos"))]
    let viewers: &[(&str, &[&str])] = &[("gcr-viewer", &[]), ("xdg-open", &[])];

    for (program, arguments) in viewers {
        match std::process::Command::new(program)
            .args(*arguments)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                // Reaps the viewer when it exits, so it does not linger as a zombie.
                std::thread::Builder::new()
                    .name("certificate-viewer".to_owned())
                    .spawn(move || child.wait())
                    .context(StartSnafu)?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(CertificateViewerError::Start { source }),
        }
    }
    Err(CertificateViewerError::NoViewer)
}
