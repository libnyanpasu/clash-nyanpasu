use std::{env, fs, path::Path};

fn extract_json_version(contents: &str) -> Option<String> {
    let (_, after_key) = contents.split_once("\"version\"")?;
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_quote = after_colon.trim_start().strip_prefix('"')?;
    let (version, _) = after_quote.split_once('"')?;
    let trimmed = version.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set");
    let root_package_json = Path::new(&manifest_dir).join("../../package.json");
    let tauri_conf_json = Path::new(&manifest_dir).join("../tauri/tauri.conf.json");

    println!("cargo:rerun-if-env-changed=NYANPASU_VERSION");
    println!("cargo:rerun-if-changed={}", root_package_json.display());
    println!("cargo:rerun-if-changed={}", tauri_conf_json.display());

    let version = env::var("NYANPASU_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            fs::read_to_string(&root_package_json)
                .ok()
                .as_deref()
                .and_then(extract_json_version)
        })
        .or_else(|| {
            fs::read_to_string(&tauri_conf_json)
                .ok()
                .as_deref()
                .and_then(extract_json_version)
        })
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set"));

    println!("cargo:rustc-env=NYANPASU_CLI_VERSION={version}");
}
