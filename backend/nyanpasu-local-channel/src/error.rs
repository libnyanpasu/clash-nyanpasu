use std::io;
#[cfg(unix)]
use std::path::PathBuf;

use snafu::Snafu;

use crate::wire::RejectReason;

/// Why a server could not be set up.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum ServerError {
    #[snafu(display(
        "the name prefix {prefix:?} must be 1 to 16 ASCII letters, digits, '-' or '_'"
    ))]
    InvalidPrefix { prefix: String },
    #[snafu(display("could not generate the connection token"))]
    Random { source: getrandom::Error },
    #[cfg(unix)]
    #[snafu(display("could not prepare the socket directory {}", path.display()))]
    SocketDir { path: PathBuf, source: io::Error },
    #[cfg(unix)]
    #[snafu(display("the socket directory {} is not private to this user", path.display()))]
    InsecureSocketDir { path: PathBuf },
    #[cfg(unix)]
    #[snafu(display("the socket path {} is longer than the platform allows", path.display()))]
    NameTooLong { path: PathBuf },
    #[cfg(unix)]
    #[snafu(display("the socket path {} is not valid UTF-8", path.display()))]
    NonUtf8Path { path: PathBuf },
    #[cfg(windows)]
    #[snafu(display("could not build the access control of the pipe"))]
    AccessControl { source: io::Error },
    #[snafu(display("could not listen on {name}"))]
    Listen { name: String, source: io::Error },
}

/// Why a server stopped waiting for its child.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum AcceptError {
    #[snafu(display("could not accept a connection"))]
    Accept { source: io::Error },
    /// A client that holds the token could not complete the handshake. It is the
    /// intended child, so waiting for another one would only hide the cause.
    #[snafu(display("the child failed the handshake"))]
    Handshake { source: HandshakeError },
}

/// Why a client could not connect.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum ConnectError {
    #[snafu(display("could not connect to {name}"))]
    Connect { name: String, source: io::Error },
    #[snafu(display("the handshake with the server failed"))]
    Handshake { source: HandshakeError },
}

/// Why a handshake failed.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum HandshakeError {
    /// The server refused the client, and told it why.
    #[snafu(display("the server rejected the connection: {reason}"))]
    Rejected { reason: RejectReason },
    /// The client uses other message types than the server.
    #[snafu(display("the peer was built with other message types"))]
    TypeMismatch,
    #[snafu(display("the peer speaks wire version {remote}, this end {local}"))]
    WireVersion { local: u16, remote: u16 },
    #[snafu(display("the peer sent a malformed handshake"))]
    Malformed,
    #[snafu(display("the connection closed during the handshake"))]
    Closed,
    #[snafu(display("I/O failed during the handshake"))]
    Io { source: io::Error },
}

/// Why a message could not be sent.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum SendError {
    /// The connection is gone: the peer closed it, exited, or an earlier write failed.
    #[snafu(display("the channel is disconnected"))]
    Disconnected,
    #[snafu(display("could not encode the message"))]
    Encode { source: postcard::Error },
    /// Nothing was written, and the channel stays usable.
    #[snafu(display("the encoded message has {len} bytes, more than the limit of {max}"))]
    FrameTooLarge { len: usize, max: u32 },
    /// A write failed for another reason than the peer leaving. The stream may
    /// have lost its place, so later sends report `Disconnected`.
    #[snafu(display("could not write the message"))]
    Io { source: io::Error },
}

/// Why a message could not be received.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum RecvError {
    /// The peer closed the connection or exited; every message it sent before
    /// that has been received.
    #[snafu(display("the channel is disconnected"))]
    Disconnected,
    /// The frame arrived whole, so the channel stays usable.
    #[snafu(display("could not decode the message"))]
    Decode { source: postcard::Error },
    /// Ends the channel: the frame was not read, so the stream has lost its place.
    #[snafu(display("the peer announced a message of {len} bytes, more than the limit of {max}"))]
    FrameTooLarge { len: usize, max: u32 },
    /// Ends the channel.
    #[snafu(display("could not read from the channel"))]
    Io { source: io::Error },
}

/// Why a [`Bootstrap`](crate::Bootstrap) could not be read.
#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum BootstrapError {
    #[snafu(display("the environment variable {var} is not set"))]
    Missing { var: String },
    #[snafu(display("the environment variable {var} is not valid Unicode"))]
    NotUnicode { var: String },
    #[snafu(display("the bootstrap is not in the form v1:<token>:<name>"))]
    Malformed,
    #[snafu(display("the bootstrap version {version:?} is not supported"))]
    UnsupportedVersion { version: String },
    #[snafu(display("the bootstrap token is not 64 hexadecimal digits"))]
    InvalidToken,
}
