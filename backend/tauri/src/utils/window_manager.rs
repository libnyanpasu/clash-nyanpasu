//! Window management utilities for multi-monitor setups with different scaling factors
use display_info::DisplayInfo;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Monitor, PhysicalPosition, PhysicalSize, Position, Size, Window};

/// Simplified monitor information for IPC
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MonitorInfo {
    pub id: usize,
    pub name: String,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub scale_factor: f64,
}

impl From<(usize, &Monitor)> for MonitorInfo {
    fn from((id, monitor): (usize, &Monitor)) -> Self {
        let position = monitor.position();
        let size = monitor.size();
        Self {
            id,
            name: format!("Monitor {}", id),
            position: (position.x, position.y),
            size: (size.width, size.height),
            scale_factor: monitor.scale_factor(),
        }
    }
}

/// Get the scale factor for a specific monitor
fn get_monitor_scale_factor(monitor: &Monitor) -> f64 {
    // Try to get the scale factor from the display info
    if let Ok(displays) = DisplayInfo::all() {
        for display in displays {
            // Match the monitor by position and size
            let monitor_pos = monitor.position();
            let monitor_size = monitor.size();

            if display.x == monitor_pos.x
                && display.y == monitor_pos.y
                && display.width as u32 == monitor_size.width
                && display.height as u32 == monitor_size.height
            {
                return display.scale_factor as f64;
            }
        }
    }

    // Fallback to the monitor's scale factor if we can't find it in display info
    monitor.scale_factor()
}

/// Move window to another monitor while correctly handling different scaling factors
pub fn move_window_to_other_monitor(
    window: Window,
    target_monitor_index: usize,
) -> tauri::Result<()> {
    let monitors = get_available_monitors(window.clone())?;

    let (_index, target_monitor) =
        monitors
            .get(target_monitor_index)
            .ok_or(tauri::Error::InvalidArgs(
                "target_monitor_index",
                "Index out of bounds",
                serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Index out of bounds",
                )),
            ))?;

    // Get current monitor
    let current_monitor = window
        .current_monitor()?
        .unwrap_or_else(|| target_monitor.clone());

    // Get scale factors for current and target monitors
    let current_scale_factor = get_monitor_scale_factor(&current_monitor);
    let target_scale_factor = get_monitor_scale_factor(target_monitor);

    // Get current window size
    let window_size = window.outer_size()?;

    // Calculate scaled size for target monitor
    let scale_ratio = current_scale_factor / target_scale_factor;
    let target_width = (window_size.width as f64 * scale_ratio).round() as u32;
    let target_height = (window_size.height as f64 * scale_ratio).round() as u32;

    // Set window size first to prevent flickering
    window.set_size(Size::Physical(PhysicalSize {
        width: target_width,
        height: target_height,
    }))?;

    // Move window to target monitor position
    let pos = target_monitor.position();
    window.set_position(Position::Physical(PhysicalPosition { x: pos.x, y: pos.y }))?;

    Ok(())
}

/// Resize window while correctly handling monitor scaling factors
fn resize_window(window: &Window, screen_share: f64) -> tauri::Result<()> {
    let monitor = window.current_monitor().unwrap().unwrap();
    let monitor_size = monitor.size();

    // Get the monitor's scale factor
    let scale_factor = get_monitor_scale_factor(&monitor);

    // Calculate size accounting for scale factor
    let scaled_size: PhysicalSize<u32> = PhysicalSize {
        width: ((monitor_size.width as f64 * screen_share) / scale_factor).round() as u32,
        height: ((monitor_size.height as f64 * screen_share) / scale_factor).round() as u32,
    };

    window.set_size(Size::Physical(scaled_size))?;
    Ok(())
}

/// Center window on current monitor while correctly handling scaling factors
#[cfg(windows)]
pub fn center_window(window: &Window) -> tauri::Result<()> {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW},
    };

    // Get current monitor
    let monitor = window.current_monitor()?.ok_or(tauri::Error::InvalidArgs(
        "current_monitor",
        "No current monitor",
        serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No current monitor",
        )),
    ))?;
    let scale_factor = get_monitor_scale_factor(&monitor);

    // Get work area
    let mut work_area = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work_area as *mut _ as *mut _, 0);
    }

    let work_area_width = (work_area.right - work_area.left) as u32;
    let work_area_height = (work_area.bottom - work_area.top) as u32;
    let work_area_x = work_area.left as i32;
    let work_area_y = work_area.top as i32;

    let window_size = window.outer_size()?;

    // Adjust for scale factor
    let adjusted_window_width = (window_size.width as f64 / scale_factor).round() as i32;
    let adjusted_window_height = (window_size.height as f64 / scale_factor).round() as i32;

    let new_x = work_area_x + (work_area_width as i32 - adjusted_window_width) / 2;
    let new_y = work_area_y + (work_area_height as i32 - adjusted_window_height) / 2;

    window.set_position(Position::Physical(PhysicalPosition { x: new_x, y: new_y }))?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn center_window(window: &Window) -> tauri::Result<()> {
    window.center();
    Ok(())
}

/// Get available monitors sorted by position
pub fn get_available_monitors(
    window: tauri::Window,
) -> tauri::Result<Vec<(usize, tauri::Monitor)>> {
    let mut monitors = window.available_monitors()?;
    monitors.sort_by(|a, b| {
        let a_pos = a.position();
        let b_pos = b.position();
        let a_size = a.size();
        let b_size = b.size();
        let a_val =
            (a_pos.y + 200) * 10 / a_size.height as i32 + (a_pos.x + 300) / a_size.width as i32;
        let b_val =
            (b_pos.y + 200) * 10 / b_size.height as i32 + (b_pos.x + 300) / b_size.width as i32;

        a_val.cmp(&b_val)
    });

    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| Ok((i, m.clone())))
        .collect()
}
