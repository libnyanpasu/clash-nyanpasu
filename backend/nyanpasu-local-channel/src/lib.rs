//! Typed, bidirectional channels between a process and the child it starts.
//!
//! The parent binds a [`tokio::OneShotServer`] and hands its [`Bootstrap`] to the
//! child, usually through the [`Bootstrap::ENV`] environment variable. The child
//! connects with [`blocking::connect`] or [`tokio::connect`], and both ends get a
//! [`Sender`](blocking::Sender) for their outgoing messages and a
//! [`Receiver`](blocking::Receiver) for the incoming ones, over one local socket:
//! a named pipe on Windows, a Unix domain socket elsewhere.
//!
//! ```no_run
//! # #[cfg(feature = "tokio")]
//! # async fn parent() -> Result<(), Box<dyn std::error::Error>> {
//! use nyanpasu_local_channel::{Bootstrap, tokio::OneShotServer};
//!
//! let (server, bootstrap) = OneShotServer::<String, u64>::new("example")?;
//! let _child = std::process::Command::new("child")
//!     .env(Bootstrap::ENV, bootstrap.to_string())
//!     .spawn()?;
//! let (tx, mut rx) = server.accept().await?;
//! tx.send(&"hello".to_owned()).await?;
//! let reply: u64 = rx.recv().await?;
//! # let _ = reply;
//! # Ok(())
//! # }
//!
//! fn child() -> Result<(), Box<dyn std::error::Error>> {
//!     use nyanpasu_local_channel::{Bootstrap, blocking};
//!
//!     let (tx, rx) = blocking::connect::<u64, String>(&Bootstrap::from_env()?)?;
//!     for message in rx {
//!         tx.send(&(message?.len() as u64))?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Security
//!
//! Only processes of the same user can reach the socket: the pipe carries a
//! DACL that grants access to the current user alone, and the Unix socket lives
//! in a directory only that user can enter. The one-time token in the bootstrap
//! guards against a stray connection reaching the server; it does not protect
//! against processes of the same user, which can read the child's environment.
//!
//! # Message types
//!
//! Messages are encoded with `postcard`, which is not self-describing, so both
//! ends must use the same types from the same build. The handshake compares the
//! type names of both directions and refuses a mismatch, which catches a child
//! built against different types; it does not version the protocol.

mod bootstrap;
mod error;
mod name;
mod wire;

pub mod blocking;
#[cfg(feature = "tokio")]
pub mod tokio;

pub use self::{
    bootstrap::Bootstrap,
    error::{
        AcceptError, BootstrapError, ConnectError, HandshakeError, RecvError, SendError,
        ServerError,
    },
    wire::RejectReason,
};

/// Settings that apply to one end of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    max_frame_len: u32,
}

impl Options {
    /// The largest encoded message either direction accepts unless configured otherwise.
    pub const DEFAULT_MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

    /// Limits the encoded size of a message this end sends or receives.
    ///
    /// A larger outgoing message fails with [`SendError::FrameTooLarge`] before
    /// anything is written. A larger incoming one fails with
    /// [`RecvError::FrameTooLarge`] and ends the channel, since the rest of the
    /// stream can no longer be trusted. Both ends normally use the same limit.
    pub const fn with_max_frame_len(self, max_frame_len: u32) -> Self {
        Self { max_frame_len }
    }

    pub const fn max_frame_len(&self) -> u32 {
        self.max_frame_len
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_frame_len: Self::DEFAULT_MAX_FRAME_LEN,
        }
    }
}

/// The message type of a direction that carries nothing.
///
/// A channel that only sends uses it for the incoming side, as in
/// `OneShotServer<Update, Never>`; no value of it can exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Never {}
