use std::{
    fs, io,
    os::unix::{
        fs::{MetadataExt as _, PermissionsExt as _, symlink},
        net::UnixListener,
    },
};

use super::{Claim, private_directory_for_user, socket_metadata_for_user, uid, validate_location};
use crate::endpoint::{
    Endpoint, EndpointError, EndpointListener,
    tests::{TEST, connect, exchange, location},
};

#[tokio::test]
async fn default_endpoint_uses_a_secure_directory_chain() {
    let directory = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::discover(TEST, directory.path(), None).unwrap();
    validate_location(endpoint.path()).unwrap();
    let parent = fs::symlink_metadata(endpoint.path().parent().unwrap()).unwrap();
    assert!(parent.is_dir());
    assert_eq!(parent.uid(), uid());
    assert_eq!(parent.mode() & 0o777, 0o700);
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let (mut client, mut server) = connect(&listener, &endpoint).await;
    exchange(&mut client, &mut server, 1).await;
}

#[tokio::test]
async fn crashed_owner_socket_is_reclaimed_only_in_private_directory() {
    let (_directory, endpoint) = location();
    let crashed = UnixListener::bind(endpoint.path()).unwrap();
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o600)).unwrap();
    drop(crashed);
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let (mut client, mut server) = connect(&listener, &endpoint).await;
    exchange(&mut client, &mut server, 3).await;
    drop(listener);
    assert!(!endpoint.path().exists());

    let crashed = UnixListener::bind(endpoint.path()).unwrap();
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o600)).unwrap();
    drop(crashed);
    let parent = endpoint.path().parent().unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::InsecurePath { .. })
    ));
    assert!(
        endpoint.path().exists(),
        "unsafe residue must be left alone"
    );
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();

    // An existing live socket without our advisory claim must also survive probing.
    fs::remove_file(endpoint.path()).unwrap();
    let live = UnixListener::bind(endpoint.path()).unwrap();
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::AlreadyServing { .. })
    ));
    assert!(endpoint.path().exists());
    drop(live);
}

#[tokio::test]
async fn inaccessible_existing_endpoint_is_refused() {
    let (_directory, endpoint) = location();
    let crashed = UnixListener::bind(endpoint.path()).unwrap();
    drop(crashed);
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::symlink_metadata(endpoint.path()).unwrap();
    let claim = Claim::acquire(endpoint.path()).unwrap();
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::NotFound,
    ] {
        let error = claim
            .reclaim_stale(|_| Err(io::Error::from(kind)))
            .unwrap_err();
        assert!(matches!(error, EndpointError::Inspect { source, .. } if source.kind() == kind));
        assert_eq!(
            fs::symlink_metadata(endpoint.path()).unwrap().ino(),
            before.ino()
        );
    }
    drop(claim);
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o000)).unwrap();
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::InsecurePath { .. })
    ));
    assert!(endpoint.path().exists());
}

#[tokio::test]
async fn local_endpoint_refuses_foreign_or_symlinked_paths() {
    let (directory, endpoint) = location();
    let parent = endpoint.path().parent().unwrap();
    assert!(matches!(
        private_directory_for_user(parent, uid().wrapping_add(1)),
        Err(EndpointError::InsecurePath { .. })
    ));
    let target = parent.join("target");
    fs::write(&target, b"untouched").unwrap();
    symlink(&target, endpoint.path()).unwrap();
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::InsecurePath { .. })
    ));
    assert_eq!(fs::read(&target).unwrap(), b"untouched");
    fs::remove_file(endpoint.path()).unwrap();
    fs::write(endpoint.path(), b"not a socket").unwrap();
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::InsecurePath { .. })
    ));
    fs::remove_file(endpoint.path()).unwrap();

    let link = parent.join("linked");
    symlink(parent, &link).unwrap();
    assert!(matches!(
        Endpoint::discover(TEST, directory.path(), Some(&link.join("rpc.sock"))),
        Err(EndpointError::InsecurePath { .. })
    ));
    fs::set_permissions(parent, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        Endpoint::discover(TEST, directory.path(), Some(endpoint.path())),
        Err(EndpointError::InsecurePath { .. })
    ));
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();

    let socket = UnixListener::bind(endpoint.path()).unwrap();
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        socket_metadata_for_user(endpoint.path(), uid().wrapping_add(1)),
        Err(EndpointError::InsecurePath { .. })
    ));
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o666)).unwrap();
    drop(socket);
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::InsecurePath { .. })
    ));
}

#[tokio::test]
async fn local_endpoint_cleanup_cannot_remove_a_replacement() {
    for replacement_socket in [false, true] {
        let (_directory, endpoint) = location();
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        // Keep the old inode alive under another name to prevent inode reuse.
        let moved = endpoint.path().with_extension("old");
        fs::rename(endpoint.path(), &moved).unwrap();
        let replacement = if replacement_socket {
            Some(UnixListener::bind(endpoint.path()).unwrap())
        } else {
            fs::write(endpoint.path(), b"replacement").unwrap();
            None
        };
        let identity = fs::symlink_metadata(endpoint.path()).unwrap();
        drop(listener);
        assert_eq!(
            fs::symlink_metadata(endpoint.path()).unwrap().ino(),
            identity.ino()
        );
        drop(replacement);
    }

    let (_directory, endpoint) = location();
    let crashed = UnixListener::bind(endpoint.path()).unwrap();
    fs::set_permissions(endpoint.path(), fs::Permissions::from_mode(0o600)).unwrap();
    drop(crashed);
    let claim = Claim::acquire(endpoint.path()).unwrap();
    let mut replacement = None;
    let result = claim.reclaim_stale(|path| {
        fs::rename(path, path.with_extension("old"))?;
        replacement = Some(UnixListener::bind(path)?);
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Err(io::Error::from(io::ErrorKind::ConnectionRefused))
    });
    assert!(matches!(result, Err(EndpointError::InsecurePath { .. })));
    assert!(
        endpoint.path().exists(),
        "stale probing must not unlink a replacement"
    );
}
