//! Backtraces a crash report can carry back to source without shipping symbols.
//!
//! A Windows release ships without its PDB, so resolving a frame there falls back
//! to the export table and names it after the nearest export before it: a name
//! that looks real and is not. Each frame is therefore printed as its module and
//! the offset of its return address in that module, which the PDB archived with
//! the build resolves offline:
//!
//! ```text
//! llvm-symbolizer --obj=clash-nyanpasu.exe --relative-address <offset>
//! ```
//!
//! A name is added only where it is trustworthy: on Windows when it comes with a
//! source location, which only debug information gives; elsewhere always, since
//! the binaries keep their symbol tables. Without a module base, as on Unix, the
//! frame is its absolute address.

use std::{ffi::c_void, fmt};

use backtrace::{Backtrace, BacktraceSymbol};

/// The frames of the panicking thread.
#[derive(Debug)]
pub struct Frames(Backtrace);

impl Frames {
    pub(crate) fn capture() -> Self {
        Self(Backtrace::new())
    }
}

impl fmt::Display for Frames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, frame) in self.0.frames().iter().enumerate() {
            let ip = frame.ip() as usize;
            match frame.module_base_address().filter(|base| !base.is_null()) {
                Some(base) => write!(
                    f,
                    "{index:>4}: {}+{:#x}",
                    module_name(base),
                    ip - base as usize
                )?,
                None => write!(f, "{index:>4}: {ip:#x}")?,
            }
            // Inlined calls resolve to several symbols, innermost first.
            for symbol in frame.symbols().iter().filter(|symbol| trusted(symbol)) {
                if let Some(name) = symbol.name() {
                    write!(f, "\n          {name:#}")?;
                }
                if let (Some(file), Some(line)) = (symbol.filename(), symbol.lineno()) {
                    write!(f, " at {}:{line}", file.display())?;
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

fn trusted(symbol: &BacktraceSymbol) -> bool {
    cfg!(not(windows)) || symbol.filename().is_some()
}

/// The file name of the module loaded at `base`.
#[cfg(windows)]
fn module_name(base: *mut c_void) -> String {
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

    let mut path = [0u16; 1024];
    // SAFETY: a module's base address is its handle, and the frame's module is
    // loaded in this process; the buffer length is the one passed.
    let len = unsafe { GetModuleFileNameW(base, path.as_mut_ptr(), path.len() as u32) } as usize;
    match String::from_utf16_lossy(&path[..len]).rsplit('\\').next() {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => format!("{base:p}"),
    }
}

#[cfg(not(windows))]
fn module_name(base: *mut c_void) -> String {
    format!("{base:p}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[inline(never)]
    fn capture_here() -> Frames {
        Frames::capture()
    }

    #[test]
    fn frames_are_numbered_from_zero() {
        let text = capture_here().to_string();
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.starts_with("          "))
            .collect();
        assert!(lines.len() > 1, "{text}");
        for (index, line) in lines.iter().enumerate() {
            assert!(line.starts_with(&format!("{index:>4}: ")), "{text}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_frames_are_offsets_into_their_module() {
        let text = capture_here().to_string();
        let test_binary = std::env::current_exe().unwrap();
        let test_binary = test_binary.file_name().unwrap().to_str().unwrap();
        assert!(
            text.lines()
                .any(|line| line.contains(&format!("{test_binary}+0x"))),
            "{text}"
        );
        // The test binary has its PDB, so the capturing function resolves with its
        // source location.
        assert!(text.contains("capture_here"), "{text}");
        assert!(text.contains("frames.rs:"), "{text}");
    }
}
