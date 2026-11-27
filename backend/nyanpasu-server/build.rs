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
        build_date(jiff::Timestamp::now())
    );
}

// Chrono RFC3339 AutoSi used 0, 3, 6 or 9 fractional digits and a numeric UTC offset.
fn build_date(timestamp: jiff::Timestamp) -> String {
    let nanos = timestamp.subsec_nanosecond();
    let format = if nanos == 0 {
        "%Y-%m-%dT%H:%M:%S+00:00"
    } else if nanos % 1_000_000 == 0 {
        "%Y-%m-%dT%H:%M:%S%.3f+00:00"
    } else if nanos % 1_000 == 0 {
        "%Y-%m-%dT%H:%M:%S%.6f+00:00"
    } else {
        "%Y-%m-%dT%H:%M:%S%.9f+00:00"
    };
    timestamp.strftime(format).to_string()
}
