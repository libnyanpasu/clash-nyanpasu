use serde::Deserialize;
use std::fs::read;
#[derive(Deserialize)]
struct PackageJson {
    version: String, // we only need the version
}

fn main() {
    let mut pkg_json = read("../../package.json").unwrap();
    let pkg_json: PackageJson = simd_json::from_slice(&mut pkg_json).unwrap();
    println!("cargo:rustc-env=NYANPASU_VERSION={}", pkg_json.version);
    tauri_build::build()
}
