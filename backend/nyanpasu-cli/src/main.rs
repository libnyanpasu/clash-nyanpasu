#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    nyanpasu_cli::run_process().await
}
