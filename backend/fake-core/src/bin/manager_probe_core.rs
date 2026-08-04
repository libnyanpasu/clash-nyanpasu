use std::{
    env,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::Path,
    process::ExitCode,
    time::Duration,
};

use fake_core::{Mode, parse_args};

const CHECK_EXIT_ENV: &str = "MANAGER_PROBE_CHECK_EXIT";

fn main() -> ExitCode {
    let args = env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|arg| arg == "-v") {
        println!("manager-probe");
        return ExitCode::SUCCESS;
    }
    match parse_args(args) {
        Ok(Mode::Check { config, .. }) => run_check(&config),
        Ok(Mode::Start { config, .. }) => run(&config),
        Err(error) => fail(error),
    }
}

fn run_check(config: &Path) -> ExitCode {
    if let Err(error) = std::fs::read_to_string(config) {
        return fail(format!(
            "failed to read config `{}`: {error}",
            config.display()
        ));
    }

    match env::var(CHECK_EXIT_ENV) {
        Ok(value) => match value.parse::<u8>() {
            Ok(code) => ExitCode::from(code),
            Err(_) => fail(format!("{CHECK_EXIT_ENV} must be an integer from 0 to 255")),
        },
        Err(env::VarError::NotPresent) => ExitCode::SUCCESS,
        Err(env::VarError::NotUnicode(_)) => fail(format!("{CHECK_EXIT_ENV} must be UTF-8")),
    }
}

fn run(config: &Path) -> ExitCode {
    let source = match std::fs::read_to_string(config) {
        Ok(source) => source,
        Err(error) => {
            return fail(format!(
                "failed to read config `{}`: {error}",
                config.display()
            ));
        }
    };
    let controller = match external_controller(&source) {
        Ok(controller) => controller,
        Err(error) => return fail(error),
    };
    let listener = match TcpListener::bind(&controller) {
        Ok(listener) => listener,
        Err(error) => return fail(format!("failed to bind {controller}: {error}")),
    };

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = respond(stream) {
                    eprintln!("manager-probe-core: request failed: {error}");
                }
            }
            Err(error) => return fail(format!("accept failed: {error}")),
        }
    }

    ExitCode::SUCCESS
}

fn external_controller(source: &str) -> Result<String, String> {
    source
        .lines()
        .find_map(|line| {
            line.trim_start()
                .strip_prefix("external-controller:")
                .map(str::trim)
        })
        .map(|value| value.trim_matches(['\'', '"']).to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "config is missing external-controller".to_owned())
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    let mut request = [0u8; 4096];
    let size = stream.read(&mut request)?;
    let first_line = String::from_utf8_lossy(&request[..size])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let is_version = first_line
        .split_whitespace()
        .take(2)
        .eq(["GET", "/version"]);
    let (status, body) = if is_version {
        ("200 OK", r#"{"version":"manager-probe"}"#)
    } else {
        ("404 Not Found", "")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    stream.shutdown(Shutdown::Write)
}

fn fail(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("manager-probe-core: {error}");
    ExitCode::from(2)
}
