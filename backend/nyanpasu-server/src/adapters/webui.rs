//! The WebUI's static files, read from a directory.

use std::path::PathBuf;

use nyanpasu_rpc::http::FrontendAssets;

/// Serves the files of a built WebUI from `root`. The HTTP asset router has
/// already refused `..` and backslashes in the request path, so a path here
/// stays under the root; a symlink inside the root is the operator's choice.
#[derive(Debug)]
pub struct DirAssets {
    root: PathBuf,
}

impl DirAssets {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl FrontendAssets for DirAssets {
    fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
        let bytes = std::fs::read(self.root.join(path)).ok()?;
        Some((mime_type(path).to_owned(), bytes))
    }
}

/// The MIME type of the file types a Vite build emits; anything else is bytes.
fn mime_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_come_from_the_root_with_a_mime_type_by_extension() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("assets")).unwrap();
        std::fs::write(directory.path().join("index.html"), "<html/>").unwrap();
        std::fs::write(directory.path().join("assets/app.JS"), "1").unwrap();
        std::fs::write(directory.path().join("assets/blob"), "2").unwrap();
        let assets = DirAssets::new(directory.path().to_owned());

        assert_eq!(
            assets.get("index.html"),
            Some(("text/html; charset=utf-8".into(), b"<html/>".to_vec()))
        );
        assert_eq!(
            assets.get("assets/app.JS").unwrap().0,
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            assets.get("assets/blob").unwrap().0,
            "application/octet-stream"
        );
        assert_eq!(assets.get("assets/missing.js"), None);
        assert_eq!(assets.get("assets"), None, "a directory is not an asset");
    }
}
