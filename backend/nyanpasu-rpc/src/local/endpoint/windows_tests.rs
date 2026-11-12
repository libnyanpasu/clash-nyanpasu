use std::{os::windows::io::AsRawHandle as _, ptr, time::Duration};

use interprocess::{
    local_socket::tokio::Stream,
    os::windows::security_descriptor::{
        AsSecurityDescriptorExt as _, BorrowedSecurityDescriptor, SecurityDescriptor,
    },
};
use tokio::io::AsyncReadExt as _;
use widestring::U16CString;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
        DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    },
};

use super::{connect, exchange, location};
use crate::local::endpoint::{Endpoint, EndpointError, EndpointListener, platform};

fn assert_current_user_acl(stream: &Stream) {
    let Stream::NamedPipe(pipe) = stream;
    let mut descriptor = ptr::null_mut();
    // SAFETY: the connected pipe owns its handle; GetSecurityInfo initializes
    // descriptor on success. The OS allocation is freed after copying it below.
    let result = unsafe {
        GetSecurityInfo(
            pipe.inner().as_raw_handle(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(result, 0, "GetSecurityInfo failed: {result}");
    // SAFETY: a successful GetSecurityInfo returns a valid security descriptor,
    // kept alive until to_owned_sd has copied it.
    let owned = unsafe { BorrowedSecurityDescriptor::from_ptr(descriptor) }.to_owned_sd();
    // SAFETY: GetSecurityInfo allocated this descriptor with LocalAlloc.
    unsafe { LocalFree(descriptor) };
    let descriptor = owned.unwrap();
    let (control, _) = descriptor.control_and_revision().unwrap();
    assert_ne!(
        control & SE_DACL_PROTECTED,
        0,
        "pipe ACL must not inherit grants"
    );
    // Compare against the same ACL round-tripped through SDDL: serialization
    // abbreviates well-known accounts (the built-in administrator becomes
    // `LA`), so the raw SID string is not always present.
    let expected = U16CString::from_str(format!(
        "D:P(A;;FA;;;{})",
        platform::current_user().unwrap()
    ))
    .unwrap();
    let expected = sddl(&SecurityDescriptor::deserialize(&expected).unwrap());
    assert_eq!(
        sddl(&descriptor),
        expected,
        "pipe ACL must grant only the current user"
    );
}

fn sddl(descriptor: &SecurityDescriptor) -> String {
    let mut text = String::new();
    descriptor
        .serialize(DACL_SECURITY_INFORMATION, |sddl| {
            text = sddl.to_string_lossy();
        })
        .unwrap();
    text
}

#[tokio::test]
async fn named_pipe_accepts_client_connected_between_accepts() {
    let (_directory, endpoint) = location();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let (mut client, mut server) = connect(&listener, &endpoint).await;
    exchange(&mut client, &mut server, 1).await;
    drop(client);
    drop(server);

    // Do not poll accept until the client has connected to the waiting instance.
    let mut client = tokio::time::timeout(Duration::from_secs(10), endpoint.connect())
        .await
        .expect("a client must connect while the listener is between accepts")
        .unwrap();
    let mut server = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the next accept must return the already connected client")
        .unwrap();
    exchange(&mut client, &mut server, 3).await;
}

#[tokio::test]
async fn named_pipe_immediate_disconnect_does_not_stop_accepting() {
    let (_directory, endpoint) = location();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let client = tokio::time::timeout(Duration::from_secs(10), endpoint.connect())
        .await
        .expect("the waiting pipe instance must accept a connection")
        .unwrap();
    let Stream::NamedPipe(pipe) = &client;
    // No bytes were written, so dropping closes the handle without deferred flush.
    pipe.inner().assume_flushed();
    drop(client);

    // Consume the disconnected client before admitting the next one. EOF proves
    // this accept did not accidentally return the subsequent live client.
    let mut disconnected = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("accept must complete even if the client has already disconnected")
        .unwrap();
    let mut byte = [0];
    let read = tokio::time::timeout(Duration::from_secs(10), disconnected.read(&mut byte))
        .await
        .expect("a disconnected client must not leave the server read pending");
    assert_eq!(read.unwrap(), 0);
    let Stream::NamedPipe(pipe) = &disconnected;
    pipe.inner().assume_flushed();
    drop(disconnected);

    let (mut client, mut server) = connect(&listener, &endpoint).await;
    exchange(&mut client, &mut server, 5).await;
}

#[tokio::test]
async fn named_pipe_accepts_reconnects_with_current_user_acl() {
    let (_directory, endpoint) = location();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    for value in 0..3 {
        let (mut client, mut server) = connect(&listener, &endpoint).await;
        assert_current_user_acl(&server);
        exchange(&mut client, &mut server, value).await;
    }
}

#[tokio::test]
async fn windows_pipe_name_competition_is_exclusive() {
    let (directory, endpoint) = location();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let (mut client, mut server) = connect(&listener, &endpoint).await;
    // A pipe name still exists while an accepted instance outlives its listener.
    drop(listener);
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::AlreadyServing { .. })
    ));
    exchange(&mut client, &mut server, 11).await;
    drop(client);
    drop(server);
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let (mut client, mut server) = connect(&listener, &endpoint).await;
    exchange(&mut client, &mut server, 13).await;
    for foreign in [r"\\.\pipe\foreign", r"\\remote\pipe\nyanpasu-rpc-test"] {
        assert!(matches!(
            Endpoint::discover(directory.path(), Some(foreign.as_ref())),
            Err(EndpointError::InvalidLocation { .. })
        ));
    }
}
