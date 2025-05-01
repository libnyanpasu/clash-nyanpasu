use chrono::{DateTime, SecondsFormat, Utc};
use rustc_version::version_meta;
use serde::Deserialize;
use std::{
    env,
    fs::{exists, read},
    process::Command,
};
#[derive(Deserialize)]
struct PackageJson {
    version: String, // we only need the version
}

#[derive(Deserialize)]
struct GitInfo {
    hash: String,
    author: String,
    time: String,
}

fn main() {
    let version: String = if let Ok(true) = exists("../../package.json") {
        let raw = read("../../package.json").unwrap();
        let pkg_json: PackageJson = serde_json::from_slice(&raw).unwrap();
        pkg_json.version
    } else {
        let raw = read("./tauri.conf.json").unwrap(); // TODO: fix it when windows arm64 need it
        let tauri_json: PackageJson = serde_json::from_slice(&raw).unwrap();
        tauri_json.version
    };
    let version = semver::Version::parse(&version).unwrap();
    let is_prerelase = !version.pre.is_empty();
    println!("cargo:rustc-env=NYANPASU_VERSION={}", version);
    // Git Information
    let (commit_hash, commit_author, commit_date) = if let Ok(true) = exists("./tmp/git-info.json")
    {
        let git_info = read("./tmp/git-info.json").unwrap();
        let git_info: GitInfo = serde_json::from_slice(&git_info).unwrap();
        (git_info.hash, git_info.author, git_info.time)
    } else {
        let output = Command::new("git")
            .args([
                "show",
                "--pretty=format:'%H,%cn,%cI'",
                "--no-patch",
                "--no-notes",
            ])
            .output()
            .expect("Failed to execute git command");
        // println!("{}", String::from_utf8(output.stderr.clone()).unwrap());
        let command_args: Vec<String> = String::from_utf8(output.stdout)
            .unwrap()
            .replace('\'', "")
            .split(',')
            .map(String::from)
            .collect();
        (
            command_args[0].clone(),
            command_args[1].clone(),
            command_args[2].clone(),
        )
    };
    println!("cargo:rustc-env=COMMIT_HASH={}", commit_hash);
    println!("cargo:rustc-env=COMMIT_AUTHOR={}", commit_author);
    let commit_date = DateTime::parse_from_rfc3339(&commit_date)
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
        if is_prerelase {
            "Nightly"
        } else {
            match env::var("PROFILE").unwrap().as_str() {
                "release" => "Release",
                "debug" => "Debug",
                _ => "Unknown",
            }
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
