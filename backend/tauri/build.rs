use chrono::{DateTime, SecondsFormat, Utc};
use rustc_version::version_meta;
use serde::Deserialize;
use std::{env, fs::read, process::Command};
#[derive(Deserialize)]
struct PackageJson {
    version: String, // we only need the version
}

fn main() {
    let mut pkg_json = read("../../package.json").unwrap();
    let pkg_json: PackageJson = simd_json::from_slice(&mut pkg_json).unwrap();
    println!("cargo:rustc-env=NYANPASU_VERSION={}", pkg_json.version);
    // Git Information
    let output = Command::new("git")
        .args([
            "show",
            "--pretty=format:'%H,%cn,%cI'",
            "--no-patch",
            "--no-notes",
        ])
        .output()
        .unwrap();
    let command_args: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .replace('\'', "")
        .split(',')
        .map(String::from)
        .collect();
    println!("cargo:rustc-env=COMMIT_HASH={}", command_args[0]);
    println!("cargo:rustc-env=COMMIT_AUTHOR={}", command_args[1]);
    let commit_date = DateTime::parse_from_rfc3339(command_args[2].as_str())
        .unwrap()
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    println!("cargo:rustc-env=COMMIT_DATE={}", commit_date);

    // Build Date
    let build_date = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    println!("cargo:rustc-env=BUILD_DATE={}", build_date);

    // Build Profile
    println!(
        "cargo:rustc-env=BUILD_PROFILE={}",
        match env::var("PROFILE").unwrap().as_str() {
            "release" => "Release",
            "debug" => "Debug",
            _ => "Unknown",
        }
    );
    // Build Platform
    println!(
        "cargo:rustc-env=BUILD_PLATFORM={}",
        env::var("TARGET").unwrap()
    );
    // Rustc Version & LLVM Version
    let rustc_version = version_meta().unwrap();
    println!(
        "cargo:rustc-env=RUSTC_VERSION={}",
        rustc_version.short_version_string
    );
    println!(
        "cargo:rustc-env=LLVM_VERSION={}",
        match rustc_version.llvm_version {
            Some(v) => v.to_string(),
            None => "Unknown".to_string(),
        }
    );
    tauri_build::build()
}
