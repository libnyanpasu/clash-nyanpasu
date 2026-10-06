use crate::{HostInputs, PathResolver};

fn resolver() -> (tempfile::TempDir, PathResolver) {
    let dir = tempfile::tempdir().unwrap();
    let resolver = PathResolver::new(HostInputs {
        app_name: "clash-nyanpasu".into(),
        executable: std::env::current_exe().map_err(std::sync::Arc::new),
        portable: false,
        development_binaries: Default::default(),
    })
    .with_roots(dir.path().join("config"), dir.path().join("data"));
    (dir, resolver)
}

#[test]
fn test_get_current_user_sid() {
    let (_dir, resolver) = resolver();
    let sid = resolver.current_user_sid();
    assert!(sid.is_ok());
    let sid = sid.unwrap();
    assert!(!sid.is_empty());
    assert!(sid.starts_with("S-"));
    println!("Current user SID: {}", sid);
}

#[test]
fn test_get_single_instance_placeholder_with_sid() {
    let (_dir, resolver) = resolver();
    let placeholder = resolver.single_instance_placeholder();
    assert!(placeholder.is_ok());
    let placeholder = placeholder.unwrap();
    assert!(!placeholder.is_empty());
    assert!(placeholder.contains("clash-nyanpasu") || placeholder.contains("clash-nyanpasu-dev"));
    println!("Single instance placeholder: {}", placeholder);
}
