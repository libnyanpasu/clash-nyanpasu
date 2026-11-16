//! Compiles the application version into the CLI: it ships inside the app bundle and always
//! reports the version of the app it came with.

use std::fs;

/// The `version` field of a JSON file, when the file exists.
fn version_of(path: &str) -> Option<String> {
    let source = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&source).ok()?;
    json["version"].as_str().map(str::to_owned)
}

fn main() {
    // `cross` mounts only the backend workspace, so the repository's package.json can be out
    // of reach; the Tauri configuration carries the same version.
    let sources = ["../../package.json", "../tauri/tauri.conf.json"];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    let version = sources
        .into_iter()
        .find_map(version_of)
        .expect("the application version is in package.json or tauri.conf.json");
    println!("cargo:rustc-env=NYANPASU_VERSION={version}");
}
