use std::path::Path;

fn main() -> anyhow::Result<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("gen");
    match std::env::args().nth(1).as_deref() {
        None => clash_nyanpasu_lib::application_api::ApplicationApi::write_generated_artifacts(
            &directory,
        ),
        Some("--check") => {
            clash_nyanpasu_lib::application_api::ApplicationApi::check_generated_artifacts(
                &directory,
            )
        }
        Some(_) => anyhow::bail!("expected no arguments or --check"),
    }
}
