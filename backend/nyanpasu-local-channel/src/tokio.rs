//! The async server and client, for processes that run a Tokio runtime.

use std::{io, marker::PhantomData};

use bytes::{Buf as _, BytesMut};
use futures_util::StreamExt as _;
use interprocess::local_socket::tokio::{Listener, RecvHalf, SendHalf, Stream, prelude::*};
use serde::{Serialize, de::DeserializeOwned};
use snafu::ResultExt as _;
use tokio::{
    io::AsyncWriteExt as _,
    sync::{mpsc, oneshot},
};
use tokio_util::codec::{Decoder, FramedRead};

use crate::{
    AcceptError, Bootstrap, ConnectError, HandshakeError, Options, RecvError, SendError,
    ServerError,
    error::{accept_error, connect_error, recv_error, server_error},
    name,
    wire::{self, ClientHello, HANDSHAKE_MAX_FRAME_LEN, PREFIX_LEN, TOKEN_LEN, Token, Verdict},
};

/// How many encoded messages wait for the background writer before `send` waits too.
const QUEUED_FRAMES: usize = 16;

/// A server that accepts exactly one client: the child it was made for.
///
/// `S` is what the server sends, `R` what it receives.
///
/// Once the server is accepted or dropped, nothing else can connect. A Unix
/// client then fails at once, since the socket file is gone. A pipe name lives
/// as long as any of its instances, including the accepted connection, and a
/// Windows client waits for a free instance; so a client after the child waits
/// until the channel closes, then fails.
pub struct OneShotServer<S, R> {
    listener: Listener,
    token: Token,
    options: Options,
    _messages: PhantomData<fn(S) -> R>,
}

impl<S, R> OneShotServer<S, R>
where
    S: Serialize,
    R: DeserializeOwned,
{
    /// Binds a server under a fresh name that starts with `prefix`, which may
    /// hold up to 16 ASCII letters, digits, `-` and `_`.
    ///
    /// The server listens from here on, so a child may be started before
    /// [`accept`](Self::accept) is called. Call it within a Tokio runtime with
    /// I/O enabled.
    pub fn new(prefix: &str) -> Result<(Self, Bootstrap), ServerError> {
        Self::with_options(prefix, Options::default())
    }

    /// [`new`](Self::new) with explicit [`Options`].
    pub fn with_options(prefix: &str, options: Options) -> Result<(Self, Bootstrap), ServerError> {
        let mut token = [0; TOKEN_LEN];
        getrandom::fill(&mut token).context(server_error::RandomSnafu)?;
        let name = name::new_name(prefix)?;
        let listener = name::listener_options(&name)?
            .create_tokio()
            .context(server_error::ListenSnafu { name: &name })?;
        let server = Self {
            listener,
            token,
            options,
            _messages: PhantomData,
        };
        Ok((server, Bootstrap::new(name, token)))
    }

    /// Waits for the child and hands out both directions of the channel.
    ///
    /// Connections that are not the child, because they hold another token or
    /// do not speak the handshake, are dropped and the wait goes on. The wait
    /// has no deadline of its own: race it against one, or against the child
    /// exiting. Dropping the future drops the server and frees its name.
    pub async fn accept(self) -> Result<(Sender<S>, Receiver<R>), AcceptError> {
        let fingerprint = wire::fingerprint::<S, R>();
        loop {
            let stream = self
                .listener
                .accept()
                .await
                .context(accept_error::AcceptSnafu)?;
            let (recv, mut send) = stream.split();
            let mut reader = FramedRead::new(recv, FrameDecoder::new(HANDSHAKE_MAX_FRAME_LEN));
            let Some(Ok(frame)) = reader.next().await else {
                continue;
            };
            let verdict = wire::judge(&frame, &self.token, fingerprint);
            let replied = match verdict.reply() {
                Some(reply) => {
                    let frame = wire::encode_frame(&reply, HANDSHAKE_MAX_FRAME_LEN)
                        .expect("a reply always fits a handshake frame");
                    send.write_all(&frame).await.is_ok()
                }
                None => false,
            };
            match verdict {
                // A child that left before reading its welcome is not waiting
                // for a channel; whoever starts next might be.
                Verdict::Accept if replied => return Ok(open(reader, send, self.options)),
                Verdict::Accept | Verdict::Ignore(_) => continue,
                Verdict::Fail(_, error) => {
                    return Err(AcceptError::Handshake { source: error });
                }
            }
        }
    }
}

/// Connects to the server `bootstrap` names. `S` is what this end sends, `R`
/// what it receives; they must be the server's types in reverse.
pub async fn connect<S, R>(bootstrap: &Bootstrap) -> Result<(Sender<S>, Receiver<R>), ConnectError>
where
    S: Serialize,
    R: DeserializeOwned,
{
    connect_with_options(bootstrap, Options::default()).await
}

/// [`connect`] with explicit [`Options`].
pub async fn connect_with_options<S, R>(
    bootstrap: &Bootstrap,
    options: Options,
) -> Result<(Sender<S>, Receiver<R>), ConnectError>
where
    S: Serialize,
    R: DeserializeOwned,
{
    let name = bootstrap.name();
    let socket_name = name::socket_name(name).context(connect_error::ConnectSnafu { name })?;
    let stream = Stream::connect(socket_name)
        .await
        .context(connect_error::ConnectSnafu { name })?;
    let (recv, mut send) = stream.split();
    let mut reader = FramedRead::new(recv, FrameDecoder::new(HANDSHAKE_MAX_FRAME_LEN));
    let hello = ClientHello::new(*bootstrap.token(), wire::fingerprint::<R, S>());
    handshake(&mut reader, &mut send, &hello)
        .await
        .context(connect_error::HandshakeSnafu)?;
    Ok(open(reader, send, options))
}

/// Both directions of a connection that has completed its handshake.
fn open<S, R>(
    mut reader: FramedRead<RecvHalf, FrameDecoder>,
    send: SendHalf,
    options: Options,
) -> (Sender<S>, Receiver<R>)
where
    S: Serialize,
    R: DeserializeOwned,
{
    reader.decoder_mut().max_frame_len = options.max_frame_len();
    let (park, parked) = oneshot::channel();
    (
        Sender::spawn(send, park, options),
        Receiver::new(reader, parked),
    )
}

async fn handshake(
    reader: &mut FramedRead<RecvHalf, FrameDecoder>,
    send: &mut SendHalf,
    hello: &ClientHello,
) -> Result<(), HandshakeError> {
    let frame = wire::encode_frame(hello, HANDSHAKE_MAX_FRAME_LEN)
        .expect("a hello always fits a handshake frame");
    send.write_all(&frame).await.map_err(|error| {
        if wire::is_disconnect(&error) {
            HandshakeError::Closed
        } else {
            HandshakeError::Io { source: error }
        }
    })?;
    match reader.next().await {
        Some(Ok(reply)) => wire::read_reply(&reply),
        Some(Err(FrameError::TooLarge { .. })) => Err(HandshakeError::Malformed),
        Some(Err(FrameError::Io(error))) if !wire::is_disconnect(&error) => {
            Err(HandshakeError::Io { source: error })
        }
        Some(Err(FrameError::Io(_))) | None => Err(HandshakeError::Closed),
    }
}

/// Sends messages of type `T`. Clones share the connection.
///
/// A background task writes the messages in the order they were queued, each
/// one whole. The connection closes once every clone and the [`Receiver`] of
/// this end are dropped; dropping only one side leaves the other working.
pub struct Sender<T> {
    queue: mpsc::Sender<(Vec<u8>, Ack)>,
    max_frame_len: u32,
    _message: PhantomData<fn(&T)>,
}

/// Tells a waiting `send` how writing its frame went.
type Ack = oneshot::Sender<io::Result<()>>;

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
            max_frame_len: self.max_frame_len,
            _message: PhantomData,
        }
    }
}

impl<T: Serialize> Sender<T> {
    fn spawn(mut half: SendHalf, park: oneshot::Sender<SendHalf>, options: Options) -> Self {
        let (queue, mut frames) = mpsc::channel::<(Vec<u8>, Ack)>(QUEUED_FRAMES);
        tokio::spawn(async move {
            while let Some((frame, ack)) = frames.recv().await {
                let written = half.write_all(&frame).await;
                // After a failed write the peer is gone, or the stream lost its
                // place part-way through a frame; nothing more can be delivered,
                // and closing the queue tells every sender so.
                let failed = written.is_err();
                let _ = ack.send(written);
                if failed {
                    break;
                }
            }
            // Dropping the write half shuts down the Unix socket for writing,
            // which the peer would read as the end of the channel while this
            // end's receiver still works. The receiver keeps it instead, until
            // it is dropped as well; if it already is, the half drops here.
            let _ = park.send(half);
        });
        Self {
            queue,
            max_frame_len: options.max_frame_len(),
            _message: PhantomData,
        }
    }

    /// Sends `message`, returning once it is written to the socket.
    ///
    /// It waits while earlier messages are being written and while the peer
    /// is not reading. Dropped before completing, it has either sent nothing
    /// or will still send the whole message; a frame is never cut short.
    pub async fn send(&self, message: &T) -> Result<(), SendError> {
        let frame = wire::encode_frame(message, self.max_frame_len)?;
        let (ack, written) = oneshot::channel();
        self.queue
            .send((frame, ack))
            .await
            .map_err(|_| SendError::Disconnected)?;
        match written.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) if !wire::is_disconnect(&error) => Err(SendError::Io { source: error }),
            // The write failed because the peer is gone, or the writer stopped
            // before this frame because an earlier one failed.
            Ok(Err(_)) | Err(_) => Err(SendError::Disconnected),
        }
    }
}

/// Receives messages of type `T`.
pub struct Receiver<T> {
    /// `None` once the channel has ended.
    reader: Option<FramedRead<RecvHalf, FrameDecoder>>,
    /// Holds the write half once every sender of this end is gone, so the
    /// connection stays open until this receiver is dropped too.
    _parked_write_half: oneshot::Receiver<SendHalf>,
    _message: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Receiver<T> {
    fn new(
        reader: FramedRead<RecvHalf, FrameDecoder>,
        parked_write_half: oneshot::Receiver<SendHalf>,
    ) -> Self {
        Self {
            reader: Some(reader),
            _parked_write_half: parked_write_half,
            _message: PhantomData,
        }
    }

    /// Waits for the next message.
    ///
    /// It is cancel safe: dropped before completing, it loses no message, so
    /// it can be raced in `select!`.
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        let reader = self.reader.as_mut().ok_or(RecvError::Disconnected)?;
        let error = match reader.next().await {
            Some(Ok(frame)) => return wire::decode(&frame).context(recv_error::DecodeSnafu),
            Some(Err(FrameError::TooLarge { len, max })) => RecvError::FrameTooLarge { len, max },
            Some(Err(FrameError::Io(error))) if !wire::is_disconnect(&error) => {
                RecvError::Io { source: error }
            }
            Some(Err(FrameError::Io(_))) | None => RecvError::Disconnected,
        };
        self.reader = None;
        Err(error)
    }
}

/// Splits the byte stream into frames, refusing any over the limit before
/// buffering it.
struct FrameDecoder {
    max_frame_len: u32,
}

impl FrameDecoder {
    fn new(max_frame_len: u32) -> Self {
        Self { max_frame_len }
    }
}

enum FrameError {
    TooLarge { len: usize, max: u32 },
    Io(io::Error),
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        FrameError::Io(error)
    }
}

impl Decoder for FrameDecoder {
    type Item = BytesMut;
    type Error = FrameError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>, FrameError> {
        let Some(prefix) = src.get(..PREFIX_LEN) else {
            return Ok(None);
        };
        let prefix = prefix.try_into().expect("the slice has the prefix length");
        let len =
            wire::payload_len(prefix, self.max_frame_len).map_err(|len| FrameError::TooLarge {
                len,
                max: self.max_frame_len,
            })?;
        let frame_len = PREFIX_LEN + len;
        if src.len() < frame_len {
            src.reserve(frame_len - src.len());
            return Ok(None);
        }
        src.advance(PREFIX_LEN);
        Ok(Some(src.split_to(len)))
    }

    /// A frame cut short means the peer went away mid-write, which the caller
    /// sees as the end of the stream rather than as an error.
    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>, FrameError> {
        let frame = self.decode(src)?;
        if frame.is_none() {
            src.clear();
        }
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoder() -> FrameDecoder {
        FrameDecoder::new(8)
    }

    #[test]
    fn frames_are_split_across_partial_and_joined_reads() {
        let mut decoder = decoder();
        let mut src = BytesMut::new();
        src.extend_from_slice(&[3, 0]);
        assert!(matches!(decoder.decode(&mut src), Ok(None)));
        src.extend_from_slice(&[0, 0, b'a', b'b']);
        assert!(matches!(decoder.decode(&mut src), Ok(None)));
        src.extend_from_slice(&[b'c', 1, 0, 0, 0, b'd']);
        let first = decoder.decode(&mut src).ok().flatten().unwrap();
        assert_eq!(&first[..], b"abc");
        let second = decoder.decode(&mut src).ok().flatten().unwrap();
        assert_eq!(&second[..], b"d");
        assert!(src.is_empty());
    }

    #[test]
    fn an_empty_frame_is_a_frame() {
        let mut src = BytesMut::from(&[0_u8, 0, 0, 0][..]);
        let frame = decoder().decode(&mut src).ok().flatten().unwrap();
        assert!(frame.is_empty());
    }

    #[test]
    fn a_frame_over_the_limit_is_refused_from_its_prefix() {
        let mut src = BytesMut::from(&9_u32.to_le_bytes()[..]);
        assert!(matches!(
            decoder().decode(&mut src),
            Err(FrameError::TooLarge { len: 9, max: 8 })
        ));
    }

    #[test]
    fn a_whole_frame_at_the_end_is_still_delivered() {
        let mut src = BytesMut::from(&[1_u8, 0, 0, 0, b'z'][..]);
        let frame = decoder().decode_eof(&mut src).ok().flatten().unwrap();
        assert_eq!(&frame[..], b"z");
    }

    #[test]
    fn a_truncated_frame_at_the_end_is_the_end_of_the_stream() {
        let mut src = BytesMut::from(&[5_u8, 0, 0, 0, b'x'][..]);
        assert!(matches!(decoder().decode_eof(&mut src), Ok(None)));
        assert!(src.is_empty());
    }
}
