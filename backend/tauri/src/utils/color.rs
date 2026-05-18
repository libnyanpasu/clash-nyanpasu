fn rgb_to_hex(red: u8, green: u8, blue: u8) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

pub fn get_system_accent_color() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Graphics::Dwm::DwmGetColorizationColor;

        let mut colorization = 0;
        let mut is_opaque = 0;

        if unsafe { DwmGetColorizationColor(&mut colorization, &mut is_opaque) } != 0 {
            return None;
        }

        Some(rgb_to_hex(
            ((colorization >> 16) & 0xff) as u8,
            ((colorization >> 8) & 0xff) as u8,
            (colorization & 0xff) as u8,
        ))
    }

    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSColor, NSColorSpace};
        use objc2_foundation::MainThreadMarker;

        fn component_to_u8(component: f64) -> u8 {
            (component.clamp(0.0, 1.0) * 255.0).round() as u8
        }

        let _mtm = MainThreadMarker::new()?;
        let color = NSColor::controlAccentColor();
        let color_space = NSColorSpace::sRGBColorSpace();
        let color = color.colorUsingColorSpace(&color_space)?;

        Some(rgb_to_hex(
            component_to_u8(color.redComponent()),
            component_to_u8(color.greenComponent()),
            component_to_u8(color.blueComponent()),
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}
