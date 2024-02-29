use std::{
    io::{BufRead, BufReader, Result, Write},
    path::Path,
};

use interprocess::local_socket::{LocalSocketListener, LocalSocketStream};
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT},
    WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY},
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use crate::ID;

pub fn register<F: FnMut(String) + Send + 'static>(scheme: &str, handler: F) -> Result<()> {
    listen(handler)?;

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

    Ok(())
}

pub fn unregister(scheme: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = Path::new("Software").join("Classes").join(scheme);

    hkcu.delete_subkey_all(base)?;

    Ok(())
}

pub fn listen<F: FnMut(String) + Send + 'static>(mut handler: F) -> Result<()> {
    std::thread::spawn(move || {
        let listener =
            LocalSocketListener::bind(ID.get().expect("listen() called before prepare()").as_str())
                .expect("Can't create listener");

        for conn in listener.incoming().filter_map(|c| {
            c.map_err(|error| log::error!("Incoming connection failed: {}", error))
                .ok()
        }) {
            // Listen for the launch arguments
            let mut conn = BufReader::new(conn);
            let mut buffer = String::new();
            if let Err(io_err) = conn.read_line(&mut buffer) {
                log::error!("Error reading incoming connection: {}", io_err.to_string());
            };
            buffer.pop();

            handler(buffer);
        }
    });

    Ok(())
}

pub fn prepare(identifier: &str) {
    if let Ok(mut conn) = LocalSocketStream::connect(identifier) {
        // We are the secondary instance.
        // Prep to activate primary instance by allowing another process to take focus.

        // A workaround to allow AllowSetForegroundWindow to succeed - press a key.
        // This was originally used by Chromium: https://bugs.chromium.org/p/chromium/issues/detail?id=837796
        dummy_keypress();

        let primary_instance_pid = conn.peer_pid().unwrap_or(ASFW_ANY);
        unsafe {
            let success = AllowSetForegroundWindow(primary_instance_pid) != 0;
            if !success {
                log::warn!("AllowSetForegroundWindow failed.");
            }
        }

        if let Err(io_err) = conn.write_all(std::env::args().nth(1).unwrap_or_default().as_bytes())
        {
            log::error!(
                "Error sending message to primary instance: {}",
                io_err.to_string()
            );
        };
        let _ = conn.write_all(b"\n");
        std::process::exit(0);
    };
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
