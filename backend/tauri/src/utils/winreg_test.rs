#[cfg(test)]
mod tests {
    use crate::utils::{dirs::get_single_instance_placeholder, winreg::get_current_user_sid};

    #[test]
    #[cfg(windows)]
    fn test_get_current_user_sid() {
        let sid = get_current_user_sid();
        assert!(sid.is_ok());
        let sid = sid.unwrap();
        assert!(!sid.is_empty());
        // SID should start with "S-" followed by numbers
        assert!(sid.starts_with("S-"));
        println!("Current user SID: {}", sid);
    }

    #[test]
    #[cfg(windows)]
    fn test_get_single_instance_placeholder_with_sid() {
        let placeholder = get_single_instance_placeholder();
        assert!(placeholder.is_ok());
        let placeholder = placeholder.unwrap();
        assert!(!placeholder.is_empty());
        // Should contain the app name
        assert!(
            placeholder.contains("clash-nyanpasu") || placeholder.contains("clash-nyanpasu-dev")
        );
        println!("Single instance placeholder: {}", placeholder);
    }
}
