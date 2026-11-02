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
                .expect("split always yields at least one component"),
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
        ID.get()
            .expect("prepare() sets the identifier before listen()")
    );
    let listener = UnixListener::bind(&addr).map_err(|error| {
        log::error!("Can't create deep-link listener at {addr}: {error}");
        error
    })?;
    std::thread::Builder::new()
        .name("deep-link".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buffer = String::new();
                        if let Err(io_err) = stream.read_to_string(&mut buffer) {
                            log::error!(
                                "Error reading incoming connection: {}",
                                io_err.to_string()
                            );
                        };

                        handler(dbg!(buffer));
                    }
                    Err(err) => {
                        log::error!("Incoming connection failed: {}", err);
                        continue;
                    }
                }
            }
        })
        .map_err(|error| {
            log::error!("Failed to start deep-link listener thread: {error}");
            error
        })?;

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
    #[test]
    fn occupied_socket_path_returns_error() {
        let identifier = format!("nyanpasu-deep-link-test-{}", std::process::id());
        let path = format!("/tmp/{identifier}-deep-link.sock");
        std::fs::create_dir(&path).unwrap();
        crate::set_identifier(&identifier).unwrap();
        let result = crate::listen(|_| {});
        std::fs::remove_dir(&path).unwrap();
        assert!(result.is_err());
    }
}
