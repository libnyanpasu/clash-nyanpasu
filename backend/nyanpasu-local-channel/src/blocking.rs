//! The blocking client, for a child that runs no async runtime, such as a GUI
//! process whose main thread belongs to its event loop.

use std::{
    io::{self, BufReader, Read, Write as _},
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use interprocess::local_socket::{RecvHalf, SendHalf, Stream, prelude::*};
use serde::{Serialize, de::DeserializeOwned};
use snafu::ResultExt as _;

use crate::{
    Bootstrap, ConnectError, HandshakeError, Options, RecvError, SendError,
    error::{connect_error, recv_error},
    name,
    wire::{self, ClientHello, HANDSHAKE_MAX_FRAME_LEN, PREFIX_LEN},
};

/// Connects to the server `bootstrap` names. `S` is what this end sends, `R`
/// what it receives; they must be the server's types in reverse.
pub fn connect<S, R>(bootstrap: &Bootstrap) -> Result<(Sender<S>, Receiver<R>), ConnectError>
where
    S: Serialize,
    R: DeserializeOwned,
{
    connect_with_options(bootstrap, Options::default())
}

/// [`connect`] with explicit [`Options`].
pub fn connect_with_options<S, R>(
    bootstrap: &Bootstrap,
    options: Options,
) -> Result<(Sender<S>, Receiver<R>), ConnectError>
where
    S: Serialize,
    R: DeserializeOwned,
{
    let name = bootstrap.name();
    let stream = name::socket_name(name)
        .and_then(Stream::connect)
        .context(connect_error::ConnectSnafu { name })?;
    let (recv, mut send) = stream.split();
    let mut reader = BufReader::new(recv);
    let hello = ClientHello::new(*bootstrap.token(), wire::fingerprint::<R, S>());
    handshake(&mut reader, &mut send, &hello).context(connect_error::HandshakeSnafu)?;
    Ok((Sender::new(send, options), Receiver::new(reader, options)))
}

fn handshake(
    reader: &mut impl Read,
    send: &mut SendHalf,
    hello: &ClientHello,
) -> Result<(), HandshakeError> {
    let frame = wire::encode_frame(hello, HANDSHAKE_MAX_FRAME_LEN)
        .expect("a hello always fits a handshake frame");
    send.write_all(&frame).map_err(|error| {
        if wire::is_disconnect(&error) {
            HandshakeError::Closed
        } else {
            HandshakeError::Io { source: error }
        }
    })?;
    match read_frame(reader, HANDSHAKE_MAX_FRAME_LEN) {
        Ok(reply) => wire::read_reply(&reply),
        Err(FrameError::Disconnected) => Err(HandshakeError::Closed),
        Err(FrameError::TooLarge { .. }) => Err(HandshakeError::Malformed),
        Err(FrameError::Io(error)) => Err(HandshakeError::Io { source: error }),
    }
}

enum FrameError {
    Disconnected,
    TooLarge { len: usize, max: u32 },
    Io(io::Error),
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        if wire::is_disconnect(&error) {
            FrameError::Disconnected
        } else {
            FrameError::Io(error)
        }
    }
}

fn read_frame(reader: &mut impl Read, max_frame_len: u32) -> Result<Vec<u8>, FrameError> {
    let mut prefix = [0; PREFIX_LEN];
    reader.read_exact(&mut prefix)?;
    let len = wire::payload_len(prefix, max_frame_len).map_err(|len| FrameError::TooLarge {
        len,
        max: max_frame_len,
    })?;
    let mut payload = vec![0; len];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

/// Sends messages of type `T`. Clones share the connection, and each message
/// is written whole even when clones send from several threads at once.
pub struct Sender<T> {
    writer: Arc<Mutex<Writer>>,
    max_frame_len: u32,
    _message: PhantomData<fn(&T)>,
}

struct Writer {
    half: SendHalf,
    /// A write failed part-way, so the peer can no longer find frame boundaries.
    broken: bool,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
            max_frame_len: self.max_frame_len,
            _message: PhantomData,
        }
    }
}

impl<T: Serialize> Sender<T> {
    fn new(half: SendHalf, options: Options) -> Self {
        Self {
            writer: Arc::new(Mutex::new(Writer {
                half,
                broken: false,
            })),
            max_frame_len: options.max_frame_len(),
            _message: PhantomData,
        }
    }

    /// Writes `message`, blocking while the peer is not reading and the pipe is full.
    pub fn send(&self, message: &T) -> Result<(), SendError> {
        let frame = wire::encode_frame(message, self.max_frame_len)?;
        // Nothing in the critical section can panic, so the lock is never poisoned.
        let mut writer = self.writer.lock().expect("no sender panics while writing");
        if writer.broken {
            return Err(SendError::Disconnected);
        }
        writer.half.write_all(&frame).map_err(|error| {
            writer.broken = true;
            if wire::is_disconnect(&error) {
                SendError::Disconnected
            } else {
                SendError::Io { source: error }
            }
        })
    }
}

/// Receives messages of type `T`.
///
/// As an [`Iterator`] it yields every message until the peer disconnects, then
/// ends; other errors are yielded before it ends.
pub struct Receiver<T> {
    /// `None` once the channel has ended.
    reader: Option<BufReader<RecvHalf>>,
    max_frame_len: u32,
    _message: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Receiver<T> {
    fn new(reader: BufReader<RecvHalf>, options: Options) -> Self {
        Self {
            reader: Some(reader),
            max_frame_len: options.max_frame_len(),
            _message: PhantomData,
        }
    }

    /// Waits for the next message.
    pub fn recv(&mut self) -> Result<T, RecvError> {
        let reader = self.reader.as_mut().ok_or(RecvError::Disconnected)?;
        let error = match read_frame(reader, self.max_frame_len) {
            Ok(payload) => return wire::decode(&payload).context(recv_error::DecodeSnafu),
            Err(FrameError::Disconnected) => RecvError::Disconnected,
            Err(FrameError::TooLarge { len, max }) => RecvError::FrameTooLarge { len, max },
            Err(FrameError::Io(error)) => RecvError::Io { source: error },
        };
        self.reader = None;
        Err(error)
    }
}

impl<T: DeserializeOwned> Iterator for Receiver<T> {
    type Item = Result<T, RecvError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.recv() {
            Err(RecvError::Disconnected) => None,
            other => Some(other),
        }
    }
}
