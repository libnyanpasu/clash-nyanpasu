#![cfg(target_os = "windows")]

use anyhow::{Result, bail};
use deelevate::{PrivilegeLevel, Token};
use runas::Command as RunasCommand;
use std::{path::Path, process::Command as StdCommand};

pub async fn invoke_uwptools(resource_dir: &Path) -> Result<()> {
    let tool_path = resource_dir.join("enableLoopback.exe");

    if !tool_path.exists() {
        bail!("enableLoopback exe not found");
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;

    match level {
        PrivilegeLevel::NotPrivileged => RunasCommand::new(tool_path).status()?,
        _ => StdCommand::new(tool_path).status()?,
    };

    Ok(())
}
