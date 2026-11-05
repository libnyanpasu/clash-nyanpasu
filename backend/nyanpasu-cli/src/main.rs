//! The `nyanpasu-cli` binary: see the library for the interface.

use std::process::ExitCode;

use nyanpasu_cli::{Cli, EXIT_FAILURE, USAGE, execute};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = match Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => return usage_error(&error),
    };
    match execute(&cli).await {
        Ok(reply) => {
            println!("{}", reply.output);
            ExitCode::from(reply.exit_code)
        }
        Err(error) => usage_error(&error),
    }
}

fn usage_error(error: &nyanpasu_cli::UsageError) -> ExitCode {
    eprintln!("nyanpasu-cli: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
    eprintln!("{USAGE}");
    ExitCode::from(EXIT_FAILURE)
}
