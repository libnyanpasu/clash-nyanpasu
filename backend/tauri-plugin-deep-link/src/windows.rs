use std::{
    path::Path,
    sync::atomic::{AtomicU16, Ordering},
};

use interprocess::{
    bound_util::RefTokioAsyncRead,
    local_socket::{
        tokio::prelude::*,
        traits::tokio::{Listener, Stream},
        GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Name, ToNsName,
    },
    os::windows::{
        local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor, ToWtf16,
    },
};
use std::io::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT},
    WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY},
    // WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY},
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use crate::ID;

pub fn register<F: FnMut(String) + Send + 'static>(schemes: &[&str], handler: F) -> Result<()> {
    listen(handler)?;

    for scheme in schemes {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let base = Path::new("Software").join("Classes").join(scheme);

        let exe = tauri_utils::platform::current_exe()?
            .display()
            .to_string()
            .replace("\\\\?\\", "");

        let (key, _) = hkcu.create_subkey(&base)?;
        key.set_value(
            "",
            &format!(
                "URL:{}",
                ID.get().expect("register() called before prepare()")
            ),
        )?;
        key.set_value("URL Protocol", &"")?;

        let (icon, _) = hkcu.create_subkey(base.join("DefaultIcon"))?;
        icon.set_value("", &format!("\"{}\",0", &exe))?;

        let (cmd, _) = hkcu.create_subkey(base.join("shell").join("open").join("command"))?;

        cmd.set_value("", &format!("\"{}\" \"%1\"", &exe))?;
    }

    Ok(())
}

pub fn unregister(schemes: &[&str]) -> Result<()> {
    for scheme in schemes {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let base = Path::new("Software").join("Classes").join(scheme);

        hkcu.delete_subkey_all(base)?;
    }

    Ok(())
}

static CRASH_COUNT: AtomicU16 = AtomicU16::new(0);

pub fn listen<F: FnMut(String) + Send + 'static>(mut handler: F) -> Result<()> {
    if CRASH_COUNT.load(Ordering::Acquire) > 5 {
        panic!("Local socket too many crashes");
    }

    std::thread::spawn(move || {
        let name = ID
            .get()
            .expect("listen() called before prepare()")
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime")
            .block_on(async move {
                let sdsf = "D:(A;;GA;;;WD)".to_wtf_16().unwrap();
                let sd = SecurityDescriptor::deserialize(&sdsf).expect("Failed to deserialize SD");
                let listener = ListenerOptions::new()
                    .name(name)
                    .nonblocking(ListenerNonblockingMode::Both)
                    .security_descriptor(sd)
                    .create_tokio()
                    .expect("Can't create listener");

                loop {
                    match listener.accept().await {
                        Ok(conn) => {
                            let (rx, mut tx) = conn.split();
                            let mut reader = BufReader::new(rx);
                            let mut buf = String::new();
                            if let Err(e) = reader.read_line(&mut buf).await {
                                log::error!("Error reading from connection: {e}");
                                continue;
                            }
                            buf.pop();
                            let current_pid = std::process::id();
                            let response = format!("{current_pid}\n");
                            if let Err(e) = tx.write_all(response.as_bytes()).await {
                                log::error!("Error writing to connection: {e}");
                                continue;
                            }
                            handler(buf);
                        }
                        Err(e) if e.raw_os_error() == Some(232) => {
                            // 234 is WSAEINTR, which means the listener was closed.
                            break;
                        }
                        Err(e) => {
                            log::error!("Error accepting connection: {e}");
                        }
                    }
                }
                CRASH_COUNT.fetch_add(1, Ordering::Release);
                let _ = listen(handler);
            });
    });

    Ok(())
}

#[inline(never)]
pub fn prepare(identifier: &str) {
    let name: Name = identifier
        .to_ns_name::<GenericNamespaced>()
        .expect("Invalid identifier");

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime")
        .block_on(async move {
            for _ in 0..3 {
                match LocalSocketStream::connect(name.clone()).await {
                    Ok(conn) => {
                        // We are the secondary instance.
                        // Prep to activate primary instance by allowing another process to take focus.

                        // A workaround to allow AllowSetForegroundWindow to succeed - press a key.
                        // This was originally used by Chromium: https://bugs.chromium.org/p/chromium/issues/detail?id=837796
                        // dummy_keypress();

                        // let primary_instance_pid = conn.peer_pid().unwrap_or(ASFW_ANY);
                        // unsafe {
                        //     let success = AllowSetForegroundWindow(primary_instance_pid) != 0;
                        //     if !success {
                        //         log::warn!("AllowSetForegroundWindow failed.");
                        //     }
                        // }
                        let (socket_rx, mut socket_tx) = conn.split();
                        let mut socket_rx = socket_rx.as_tokio_async_read();
                        let url = std::env::args().nth(1).expect("URL not provided");
                        socket_tx
                            .write_all(url.as_bytes())
                            .await
                            .expect("Failed to write to socket");
                        socket_tx
                            .write_all(b"\n")
                            .await
                            .expect("Failed to write to socket");
                        socket_tx.flush().await.expect("Failed to flush socket");

                        let mut reader = BufReader::new(&mut socket_rx);
                        let mut buf = String::new();
                        if let Err(e) = reader.read_line(&mut buf).await {
                            eprintln!("Error reading from connection: {e}");
                        }
                        buf.pop();
                        dummy_keypress();
                        let pid = buf.parse::<u32>().unwrap_or(ASFW_ANY);
                        unsafe {
                            let success = AllowSetForegroundWindow(pid) != 0;
                            if !success {
                                eprintln!("AllowSetForegroundWindow failed.");
                            }
                        }
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to local socket: {e}");
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                };
            }
        });

    ID.set(identifier.to_string())
        .expect("prepare() called more than once with different identifiers.");
}

/// Send a dummy keypress event so AllowSetForegroundWindow can succeed
fn dummy_keypress() {
    let keyboard_input_down = KEYBDINPUT {
        wVk: 0, // This doesn't correspond to any actual keyboard key, but should still function for the workaround.
        dwExtraInfo: 0,
        wScan: 0,
        time: 0,
        dwFlags: 0,
    };

    let mut keyboard_input_up = keyboard_input_down;
    keyboard_input_up.dwFlags = 0x0002; // KEYUP flag

    let input_down_u = INPUT_0 {
        ki: keyboard_input_down,
    };
    let input_up_u = INPUT_0 {
        ki: keyboard_input_up,
    };

    let input_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: input_down_u,
    };

    let input_up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: input_up_u,
    };

    let ipsize = std::mem::size_of::<INPUT>() as i32;
    unsafe {
        SendInput(2, [input_down, input_up].as_ptr(), ipsize);
    };
}
