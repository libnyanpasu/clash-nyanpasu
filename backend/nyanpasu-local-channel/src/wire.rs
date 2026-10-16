//! What travels on the socket: length-prefixed frames, and the handshake that
//! opens every connection.
//!
//! A frame is a little-endian `u32` length followed by that many bytes of
//! postcard. The client opens with a [`ClientHello`] frame and the server answers
//! with a [`ServerReply`] frame before either side sends messages.

use std::{any::type_name, fmt, io};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::ResultExt as _;

use crate::error::{HandshakeError, SendError, send_error};

pub(crate) const PREFIX_LEN: usize = 4;
pub(crate) const MAGIC: [u8; 4] = *b"NYCH";
pub(crate) const WIRE_VERSION: u16 = 1;
pub(crate) const TOKEN_LEN: usize = 32;
/// The handshake frames are a few dozen bytes; anything larger is not a client.
pub(crate) const HANDSHAKE_MAX_FRAME_LEN: u32 = 256;

pub(crate) type Token = [u8; TOKEN_LEN];

/// Why the server refused a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectReason {
    /// The client presented another token: it was not started with this server's bootstrap.
    Token,
    /// The client speaks another wire version.
    WireVersion,
    /// The client uses other message types.
    Types,
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RejectReason::Token => "wrong token",
            RejectReason::WireVersion => "unsupported wire version",
            RejectReason::Types => "mismatched message types",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ClientHello {
    pub(crate) magic: [u8; 4],
    pub(crate) wire: u16,
    pub(crate) token: Token,
    pub(crate) fingerprint: u64,
}

impl ClientHello {
    pub(crate) fn new(token: Token, fingerprint: u64) -> Self {
        Self {
            magic: MAGIC,
            wire: WIRE_VERSION,
            token,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ServerReply {
    Accepted,
    Rejected(RejectReason),
}

/// What the server makes of one connection's opening frame.
#[cfg(any(feature = "tokio", test))]
#[derive(Debug)]
pub(crate) enum Verdict {
    /// The child: reply `Accepted` and hand out the channel.
    Accept,
    /// Not this server's child, or not a client at all: reply if there is
    /// something to say, then keep waiting.
    Ignore(Option<RejectReason>),
    /// The child, but it cannot talk to this server: reply, then fail the accept.
    Fail(RejectReason, HandshakeError),
}

#[cfg(any(feature = "tokio", test))]
impl Verdict {
    pub(crate) fn reply(&self) -> Option<ServerReply> {
        match self {
            Verdict::Accept => Some(ServerReply::Accepted),
            Verdict::Ignore(reason) => reason.map(ServerReply::Rejected),
            Verdict::Fail(reason, _) => Some(ServerReply::Rejected(*reason)),
        }
    }
}

/// Judges an opening frame. The token is checked before anything else the
/// client claims, so only the child learns why it was refused.
#[cfg(any(feature = "tokio", test))]
pub(crate) fn judge(frame: &[u8], token: &Token, fingerprint: u64) -> Verdict {
    let Ok(hello) = postcard::from_bytes::<ClientHello>(frame) else {
        return Verdict::Ignore(None);
    };
    if hello.magic != MAGIC {
        return Verdict::Ignore(None);
    }
    if !tokens_equal(&hello.token, token) {
        return Verdict::Ignore(Some(RejectReason::Token));
    }
    if hello.wire != WIRE_VERSION {
        return Verdict::Fail(
            RejectReason::WireVersion,
            HandshakeError::WireVersion {
                local: WIRE_VERSION,
                remote: hello.wire,
            },
        );
    }
    if hello.fingerprint != fingerprint {
        return Verdict::Fail(RejectReason::Types, HandshakeError::TypeMismatch);
    }
    Verdict::Accept
}

/// What the client makes of the server's reply.
pub(crate) fn read_reply(frame: &[u8]) -> Result<(), HandshakeError> {
    match postcard::from_bytes::<ServerReply>(frame) {
        Ok(ServerReply::Accepted) => Ok(()),
        Ok(ServerReply::Rejected(reason)) => Err(HandshakeError::Rejected { reason }),
        Err(_) => Err(HandshakeError::Malformed),
    }
}

/// Identifies the message types of a channel, in server order: `Down` is what
/// the server sends, `Up` what it receives.
///
/// Type names are stable within one build, which is all a channel between a
/// parent and the child it starts needs; across builds they are not promised.
pub(crate) fn fingerprint<Down, Up>() -> u64 {
    let mut hash = Fnv1a::new();
    hash.write(type_name::<Down>().as_bytes());
    // A separator, so that ("ab", "c") and ("a", "bc") differ.
    hash.write(&[0xff]);
    hash.write(type_name::<Up>().as_bytes());
    hash.finish()
}

struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(Self::PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Compares in time independent of where the tokens differ.
#[cfg(any(feature = "tokio", test))]
pub(crate) fn tokens_equal(a: &Token, b: &Token) -> bool {
    let difference = a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(difference) == 0
}

/// Encodes `message` as one frame, prefix included, so a writer sends it with a
/// single write.
pub(crate) fn encode_frame<T: Serialize + ?Sized>(
    message: &T,
    max_frame_len: u32,
) -> Result<Vec<u8>, SendError> {
    let mut frame =
        postcard::to_extend(message, vec![0; PREFIX_LEN]).context(send_error::EncodeSnafu)?;
    let len = frame.len() - PREFIX_LEN;
    let prefix = u32::try_from(len)
        .ok()
        .filter(|len| *len <= max_frame_len)
        .ok_or(SendError::FrameTooLarge {
            len,
            max: max_frame_len,
        })?;
    frame[..PREFIX_LEN].copy_from_slice(&prefix.to_le_bytes());
    Ok(frame)
}

/// The payload length a prefix announces, if it is within the limit.
pub(crate) fn payload_len(prefix: [u8; PREFIX_LEN], max_frame_len: u32) -> Result<usize, usize> {
    let len = u32::from_le_bytes(prefix);
    let as_usize = len as usize;
    if len <= max_frame_len {
        Ok(as_usize)
    } else {
        Err(as_usize)
    }
}

pub(crate) fn decode<T: DeserializeOwned>(payload: &[u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(payload)
}

/// Whether an I/O error means the peer is gone rather than something else failing.
pub(crate) fn is_disconnect(error: &io::Error) -> bool {
    use io::ErrorKind::*;
    if matches!(
        error.kind(),
        BrokenPipe | ConnectionReset | ConnectionAborted | NotConnected | UnexpectedEof
    ) {
        return true;
    }
    // ERROR_BROKEN_PIPE, ERROR_NO_DATA and ERROR_PIPE_NOT_CONNECTED: the three
    // ways a named pipe reports that its other end has closed.
    cfg!(windows) && matches!(error.raw_os_error(), Some(109 | 232 | 233))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: Token = [7; TOKEN_LEN];

    fn hello_frame(hello: &ClientHello) -> Vec<u8> {
        postcard::to_allocvec(hello).unwrap()
    }

    #[test]
    fn a_matching_hello_is_accepted() {
        let hello = ClientHello::new(TOKEN, 42);
        assert!(matches!(
            judge(&hello_frame(&hello), &TOKEN, 42),
            Verdict::Accept
        ));
        assert_eq!(Verdict::Accept.reply(), Some(ServerReply::Accepted));
    }

    #[test]
    fn bytes_that_are_not_a_hello_are_ignored_without_a_reply() {
        for frame in [&b""[..], b"\xff\xff\xff", &[0; 64]] {
            let verdict = judge(frame, &TOKEN, 42);
            assert!(matches!(verdict, Verdict::Ignore(None)), "{frame:?}");
            assert_eq!(verdict.reply(), None);
        }
    }

    #[test]
    fn a_hello_with_other_magic_is_ignored_without_a_reply() {
        let mut hello = ClientHello::new(TOKEN, 42);
        hello.magic = *b"HTTP";
        assert!(matches!(
            judge(&hello_frame(&hello), &TOKEN, 42),
            Verdict::Ignore(None)
        ));
    }

    #[test]
    fn a_wrong_token_is_rejected_and_the_server_keeps_waiting() {
        let mut hello = ClientHello::new(TOKEN, 42);
        hello.token[31] ^= 1;
        let verdict = judge(&hello_frame(&hello), &TOKEN, 42);
        assert!(matches!(
            verdict,
            Verdict::Ignore(Some(RejectReason::Token))
        ));
        assert_eq!(
            verdict.reply(),
            Some(ServerReply::Rejected(RejectReason::Token))
        );
    }

    #[test]
    fn the_token_is_checked_before_the_version_and_types() {
        let mut hello = ClientHello::new([0; TOKEN_LEN], 0);
        hello.wire = WIRE_VERSION + 1;
        assert!(matches!(
            judge(&hello_frame(&hello), &TOKEN, 42),
            Verdict::Ignore(Some(RejectReason::Token))
        ));
    }

    #[test]
    fn the_child_with_another_wire_version_fails_the_accept() {
        let mut hello = ClientHello::new(TOKEN, 42);
        hello.wire = WIRE_VERSION + 1;
        let verdict = judge(&hello_frame(&hello), &TOKEN, 42);
        assert!(
            matches!(
                verdict,
                Verdict::Fail(RejectReason::WireVersion, HandshakeError::WireVersion { local, remote })
                    if local == WIRE_VERSION && remote == WIRE_VERSION + 1
            ),
            "{verdict:?}"
        );
        assert_eq!(
            verdict.reply(),
            Some(ServerReply::Rejected(RejectReason::WireVersion))
        );
    }

    #[test]
    fn the_child_with_other_types_fails_the_accept() {
        let hello = ClientHello::new(TOKEN, 41);
        let verdict = judge(&hello_frame(&hello), &TOKEN, 42);
        assert!(
            matches!(
                verdict,
                Verdict::Fail(RejectReason::Types, HandshakeError::TypeMismatch)
            ),
            "{verdict:?}"
        );
    }

    #[test]
    fn replies_are_read_back() {
        let accepted = postcard::to_allocvec(&ServerReply::Accepted).unwrap();
        assert!(read_reply(&accepted).is_ok());
        for reason in [
            RejectReason::Token,
            RejectReason::WireVersion,
            RejectReason::Types,
        ] {
            let frame = postcard::to_allocvec(&ServerReply::Rejected(reason)).unwrap();
            assert!(
                matches!(read_reply(&frame), Err(HandshakeError::Rejected { reason: r }) if r == reason)
            );
        }
        assert!(matches!(
            read_reply(&[9, 9]),
            Err(HandshakeError::Malformed)
        ));
    }

    #[test]
    fn a_rejection_says_why() {
        for (reason, why) in [
            (RejectReason::Token, "wrong token"),
            (RejectReason::WireVersion, "unsupported wire version"),
            (RejectReason::Types, "mismatched message types"),
        ] {
            assert_eq!(
                HandshakeError::Rejected { reason }.to_string(),
                format!("the server rejected the connection: {why}")
            );
        }
    }

    #[test]
    fn the_fingerprint_depends_on_both_directions_and_their_order() {
        let base = fingerprint::<String, u64>();
        assert_eq!(base, fingerprint::<String, u64>());
        assert_ne!(base, fingerprint::<u64, String>());
        assert_ne!(base, fingerprint::<String, u32>());
        assert_ne!(base, fingerprint::<Vec<String>, u64>());
    }

    #[test]
    fn tokens_compare_by_every_byte() {
        let mut other = TOKEN;
        assert!(tokens_equal(&TOKEN, &other));
        for index in [0, 15, 31] {
            other = TOKEN;
            other[index] ^= 0x80;
            assert!(!tokens_equal(&TOKEN, &other), "byte {index}");
        }
    }

    #[test]
    fn a_frame_carries_its_payload_length_little_endian() {
        let frame = encode_frame(&"héllo".to_owned(), 64).unwrap();
        let payload = &frame[PREFIX_LEN..];
        let prefix: [u8; PREFIX_LEN] = frame[..PREFIX_LEN].try_into().unwrap();
        assert_eq!(payload_len(prefix, 64), Ok(payload.len()));
        assert_eq!(decode::<String>(payload).unwrap(), "héllo");
    }

    #[test]
    fn a_message_over_the_limit_is_not_encoded() {
        let message = vec![0_u8; 100];
        let error = encode_frame(&message, 64).unwrap_err();
        assert!(
            matches!(error, SendError::FrameTooLarge { len, max: 64 } if len > 100),
            "{error:?}"
        );
        assert!(encode_frame(&message, 200).is_ok());
    }

    #[test]
    fn a_prefix_over_the_limit_is_refused() {
        assert_eq!(payload_len(64_u32.to_le_bytes(), 64), Ok(64));
        assert_eq!(payload_len(65_u32.to_le_bytes(), 64), Err(65));
        assert_eq!(
            payload_len(u32::MAX.to_le_bytes(), 64),
            Err(u32::MAX as usize)
        );
    }

    #[test]
    fn closed_pipes_count_as_disconnects_and_other_errors_do_not() {
        for kind in [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert!(is_disconnect(&io::Error::from(kind)), "{kind:?}");
        }
        assert!(!is_disconnect(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        #[cfg(windows)]
        for code in [109, 232, 233] {
            assert!(is_disconnect(&io::Error::from_raw_os_error(code)), "{code}");
        }
    }
}
