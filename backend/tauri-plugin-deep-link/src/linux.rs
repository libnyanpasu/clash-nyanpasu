use std::{
    fs::{create_dir_all, remove_file, File},
    io::{Error, ErrorKind, Read, Result, Write},
    os::unix::net::{UnixListener, UnixStream},
    process::Command,
};

use dirs::data_dir;

use crate::ID;

pub fn register<F: FnMut(String) + Send + 'static>(schemes: &[&str], handler: F) -> Result<()> {
    listen(handler)?;

    let mut target = data_dir()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "data directory not found."))?
        .join("applications");

    create_dir_all(&target)?;

    let exe = tauri_utils::platform::current_exe()?;

    let file_name = format!(
        "{}-handler.desktop",
        exe.file_name()
            .ok_or_else(|| Error::new(
                ErrorKind::NotFound,
                "Couldn't get file name of curent executable.",
            ))?
            .to_string_lossy()
    );

    target.push(&file_name);

    let mime_types = format!(
        "{};",
        schemes
            .iter()
            .map(|s| format!("x-scheme-handler/{}", s))
            .collect::<Vec<String>>()
            .join(";")
    );

    let mut file = File::create(&target)?;
    file.write_all(
        format!(
            include_str!("template.desktop"),
            name = ID
                .get()
                .expect("Called register() before prepare()")
                .split('.')
                .last()
                .unwrap(),
            exec = std::env::var("APPIMAGE").unwrap_or_else(|_| exe.display().to_string()),
            mime_types = mime_types
        )
        .as_bytes(),
    )?;

    // update-desktop-database [-q|--quiet] [-v|--verbose] [DIRECTORY...]
    target.pop();

    Command::new("update-desktop-database")
        .arg(&target)
        .status()?;

    for scheme in schemes {
        Command::new("xdg-mime")
            .args([
                "default",
                &file_name,
                &format!("x-scheme-handler/{}", scheme),
            ])
            .status()?;
    }

    Ok(())
}

pub fn unregister(_schemes: &[&str]) -> Result<()> {
    let mut target =
        data_dir().ok_or_else(|| Error::new(ErrorKind::NotFound, "data directory not found."))?;

    target.push("applications");

    target.push(format!(
        "{}-handler.desktop",
        tauri_utils::platform::current_exe()?
            .file_name()
            .ok_or_else(|| Error::new(
                ErrorKind::NotFound,
                "Couldn't get file name of current executable.",
            ))?
            .to_string_lossy()
    ));

    remove_file(&target)?;
    target.pop();

    Ok(())
}

pub fn listen<F: FnMut(String) + Send + 'static>(mut handler: F) -> Result<()> {
    let addr = format!(
        "/tmp/{}-deep-link.sock",
        ID.get().expect("listen() called before prepare()")
    );
    let listener = UnixListener::bind(&addr)?;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let mut buffer = String::new();
                    if let Err(io_err) = stream.read_to_string(&mut buffer) {
                        log::error!("Error reading incoming connection: {}", io_err.to_string());
                    };

                    handler(dbg!(buffer));
                }
                Err(err) => {
                    log::error!("Incoming connection failed: {}", err);
                    continue;
                }
            }
        }
    });

    Ok(())
}

pub fn prepare(identifier: &str) {
    let addr = format!("/tmp/{}-deep-link.sock", identifier);

    match UnixStream::connect(&addr) {
        Ok(mut stream) => {
            if let Err(io_err) =
                stream.write_all(std::env::args().nth(1).unwrap_or_default().as_bytes())
            {
                log::error!(
                    "Error sending message to primary instance: {}",
                    io_err.to_string()
                );
            };
            std::process::exit(0);
        }
        Err(err) => {
            log::error!("Error creating socket listener: {}", err.to_string());
            if err.kind() == ErrorKind::ConnectionRefused {
                let _ = remove_file(&addr);
            }
        }
    };
    ID.set(identifier.to_string())
        .expect("prepare() called more than once with different identifiers.");
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{ErrorKind, Write},
        os::unix::net::{UnixListener, UnixStream},
        process::Command,
        sync::mpsc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn listen_reports_bind_errors_and_delivers_urls() {
        const CHILD_CASE: &str = "NYANPASU_DEEP_LINK_TEST_CASE";
        const CHILD_ID: &str = "NYANPASU_DEEP_LINK_TEST_ID";

        // Each child gets its own once-set identifier and listener lifetime.
        let Ok(case) = std::env::var(CHILD_CASE) else {
            for case in ["occupied", "stale", "file", "healthy"] {
                let identifier = format!(
                    "nyanpasu-test-{}-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                    case
                );
                let addr = format!("/tmp/{}-deep-link.sock", identifier);
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "platform_impl::tests::listen_reports_bind_errors_and_delivers_urls",
                        "--nocapture",
                    ])
                    .env(CHILD_CASE, case)
                    .env(CHILD_ID, identifier)
                    .output();
                // Clean up even when the child fails an assertion or panics.
                let cleanup = fs::remove_file(&addr);
                if let Err(err) = cleanup {
                    assert_eq!(err.kind(), ErrorKind::NotFound);
                }
                let output = output.unwrap();
                assert!(output.status.success(), "{case}: {output:?}");
            }
            return;
        };

        let identifier = std::env::var(CHILD_ID).unwrap();
        crate::set_identifier(&identifier).unwrap();
        let addr = format!("/tmp/{}-deep-link.sock", identifier);
        let occupied = match case.as_str() {
            "occupied" => Some(UnixListener::bind(&addr).unwrap()),
            "stale" => {
                drop(UnixListener::bind(&addr).unwrap());
                None
            }
            "file" => {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&addr)
                    .unwrap();
                file.write_all(b"keep this file").unwrap();
                None
            }
            "healthy" => None,
            _ => panic!("unknown listener test case: {case}"),
        };
        let (sender, receiver) = mpsc::channel();
        let result = crate::listen(move |url| sender.send(url).unwrap());

        if case == "healthy" {
            result.unwrap();
            let url = "clash-nyanpasu://install-config?url=https://example.com/config.yaml";
            let mut stream = UnixStream::connect(&addr).unwrap();
            stream.write_all(url.as_bytes()).unwrap();
            drop(stream);
            assert_eq!(receiver.recv_timeout(Duration::from_secs(5)).unwrap(), url);
        } else {
            assert_eq!(result.unwrap_err().kind(), ErrorKind::AddrInUse);
            assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected));
            if let Some(listener) = occupied {
                listener.set_nonblocking(true).unwrap();
                let _stream = UnixStream::connect(&addr).unwrap();
                listener.accept().unwrap();
            }
            if case == "file" {
                assert_eq!(fs::read(&addr).unwrap(), b"keep this file");
            }
        }
    }
}
