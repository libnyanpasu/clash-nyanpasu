fn main() {
    println!("cargo:rerun-if-changed=../../package.json");
    let package: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("../../package.json").expect("read application package metadata"),
    )
    .expect("parse application package metadata");
    println!(
        "cargo:rustc-env=NYANPASU_VERSION={}",
        package["version"].as_str().expect("application version")
    );
    println!(
        "cargo:rustc-env=BUILD_DATE={}",
        chrono::Utc::now().to_rfc3339()
    );
}
