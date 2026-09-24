use nyanpasu_openapi_p0_probe::{FakeApplication, application_api};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (_, openapi) = application_api(FakeApplication::default());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json");
    std::fs::write(path, openapi.to_pretty_json()?)?;
    println!("Wrote {path}");
    Ok(())
}
