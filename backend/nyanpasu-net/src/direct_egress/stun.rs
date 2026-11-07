//! The part of STUN (RFC 8489) the probe needs: a Binding request and the
//! mapped address in its success response. No authentication and no
//! `SOFTWARE` attribute, so a server learns nothing about the client.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub(super) const HEADER_LEN: usize = 20;
const MAGIC_COOKIE: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const FAMILY_IPV4: u8 = 0x01;
const FAMILY_IPV6: u8 = 0x02;

/// The 96-bit identifier that ties a response to its request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TransactionId([u8; 12]);

impl TransactionId {
    pub(super) fn random() -> Result<Self, getrandom::Error> {
        let mut bytes = [0; 12];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes))
    }
}

#[derive(Debug, PartialEq, Eq, snafu::Snafu)]
pub(super) enum DecodeError {
    #[snafu(display("the message is shorter than its header or its declared length"))]
    Truncated,
    #[snafu(display("the message is not a STUN message"))]
    NotStun,
    #[snafu(display("the message answers another request"))]
    OtherTransaction,
    #[snafu(display("the message is not a Binding success response: {message_type:#06x}"))]
    NotBindingSuccess { message_type: u16 },
    #[snafu(display("the response carries no mapped address"))]
    NoMappedAddress,
}

#[must_use]
pub(super) fn binding_request(id: TransactionId) -> [u8; HEADER_LEN] {
    let mut message = [0; HEADER_LEN];
    message[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Bytes 2..4 are the attribute length, which is 0.
    message[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    message[8..20].copy_from_slice(&id.0);
    message
}

/// How many bytes follow a header. TCP reads the rest of a message by it.
#[must_use]
pub(super) fn body_len(header: &[u8; HEADER_LEN]) -> usize {
    usize::from(u16::from_be_bytes([header[2], header[3]]))
}

/// The address the server saw the request come from. `XOR-MAPPED-ADDRESS`
/// wins over the older `MAPPED-ADDRESS`, which some servers still send.
pub(super) fn mapped_address(message: &[u8], id: TransactionId) -> Result<SocketAddr, DecodeError> {
    let header: &[u8; HEADER_LEN] = message
        .get(..HEADER_LEN)
        .and_then(|header| header.try_into().ok())
        .ok_or(DecodeError::Truncated)?;
    let cookie = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    // The top two bits of every STUN message are zero.
    if cookie != MAGIC_COOKIE || header[0] & 0xC0 != 0 {
        return Err(DecodeError::NotStun);
    }
    if header[8..20] != id.0 {
        return Err(DecodeError::OtherTransaction);
    }
    let message_type = u16::from_be_bytes([header[0], header[1]]);
    if message_type != BINDING_SUCCESS {
        return Err(DecodeError::NotBindingSuccess { message_type });
    }
    let mut attributes = message
        .get(HEADER_LEN..HEADER_LEN + body_len(header))
        .ok_or(DecodeError::Truncated)?;

    let mut mapped = None;
    while attributes.len() >= 4 {
        let kind = u16::from_be_bytes([attributes[0], attributes[1]]);
        let len = usize::from(u16::from_be_bytes([attributes[2], attributes[3]]));
        let value = attributes.get(4..4 + len).ok_or(DecodeError::Truncated)?;
        match kind {
            XOR_MAPPED_ADDRESS => {
                if let Some(address) = decode_address(value, Some(id)) {
                    return Ok(address);
                }
            }
            MAPPED_ADDRESS => mapped = mapped.or(decode_address(value, None)),
            _ => {}
        }
        // Attribute values are padded to a multiple of four bytes.
        let padded = (4 + len).next_multiple_of(4);
        attributes = attributes.get(padded..).unwrap_or_default();
    }
    mapped.ok_or(DecodeError::NoMappedAddress)
}

/// `None` for a malformed value. `xor` carries the transaction ID when the
/// value is XOR-ed with the magic cookie and that ID.
fn decode_address(value: &[u8], xor: Option<TransactionId>) -> Option<SocketAddr> {
    let family = *value.get(1)?;
    let mut port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?]);
    // An all-zero key leaves a plain `MAPPED-ADDRESS` as it is.
    let mut key = [0; 16];
    if let Some(id) = xor {
        key[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
        key[4..].copy_from_slice(&id.0);
        port ^= (MAGIC_COOKIE >> 16) as u16;
    }
    let unmask = |bytes: &[u8]| -> Vec<u8> {
        bytes
            .iter()
            .zip(key)
            .map(|(byte, mask)| byte ^ mask)
            .collect()
    };
    let ip = match family {
        FAMILY_IPV4 => {
            let octets: [u8; 4] = unmask(value.get(4..8)?).try_into().ok()?;
            IpAddr::V4(Ipv4Addr::from(octets))
        }
        FAMILY_IPV6 => {
            let octets: [u8; 16] = unmask(value.get(4..20)?).try_into().ok()?;
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The IPv4 sample response of RFC 5769 §2.2: `SOFTWARE`,
    /// `XOR-MAPPED-ADDRESS` 192.0.2.1:32853, `MESSAGE-INTEGRITY`, `FINGERPRINT`.
    const RFC_5769_ID: TransactionId = TransactionId([
        0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
    ]);
    #[rustfmt::skip] // One attribute per line, as the RFC prints them.
    const RFC_5769_RESPONSE: [u8; 80] = [
        0x01, 0x01, 0x00, 0x3c, 0x21, 0x12, 0xa4, 0x42,
        0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
        0x80, 0x22, 0x00, 0x0b, b't', b'e', b's', b't', b' ', b'v', b'e', b'c', b't', b'o', b'r', b' ',
        0x00, 0x20, 0x00, 0x08, 0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43,
        0x00, 0x08, 0x00, 0x14, 0x2b, 0x91, 0xf5, 0x99, 0xfd, 0x9e, 0x90, 0xc3, 0x8c, 0x74,
        0x89, 0xf9, 0x2a, 0xf9, 0xba, 0x53, 0xf0, 0x6b, 0xe7, 0xd7,
        0x80, 0x28, 0x00, 0x04, 0xc0, 0x7d, 0x4c, 0x96,
    ];

    /// A Binding success response naming `address`, as a server would send it.
    pub(in super::super) fn binding_response(request: &[u8], address: SocketAddr) -> Vec<u8> {
        let id: [u8; 12] = request[8..20].try_into().unwrap();
        let mut key = MAGIC_COOKIE.to_be_bytes().to_vec();
        key.extend_from_slice(&id);
        let (family, octets) = match address.ip() {
            IpAddr::V4(ip) => (FAMILY_IPV4, ip.octets().to_vec()),
            IpAddr::V6(ip) => (FAMILY_IPV6, ip.octets().to_vec()),
        };
        let mut value = vec![0, family];
        value.extend_from_slice(&(address.port() ^ (MAGIC_COOKIE >> 16) as u16).to_be_bytes());
        value.extend(octets.iter().zip(&key).map(|(byte, mask)| byte ^ mask));

        let mut message = Vec::new();
        message.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
        message.extend_from_slice(&(4 + value.len() as u16).to_be_bytes());
        message.extend_from_slice(&request[4..20]);
        message.extend_from_slice(&XOR_MAPPED_ADDRESS.to_be_bytes());
        message.extend_from_slice(&(value.len() as u16).to_be_bytes());
        message.extend_from_slice(&value);
        message
    }

    #[test]
    fn a_request_is_a_bare_binding_header() {
        let request = binding_request(RFC_5769_ID);

        assert_eq!(
            request[..8],
            [0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xa4, 0x42]
        );
        assert_eq!(request[8..], RFC_5769_ID.0);
    }

    #[test]
    fn the_rfc_5769_sample_response_maps_to_its_address() {
        assert_eq!(
            mapped_address(&RFC_5769_RESPONSE, RFC_5769_ID),
            Ok("192.0.2.1:32853".parse().unwrap())
        );
        assert_eq!(body_len(RFC_5769_RESPONSE[..20].try_into().unwrap()), 60);
    }

    #[test]
    fn ipv6_addresses_are_unmasked_with_the_transaction_id() {
        let id = TransactionId::random().unwrap();
        let address: SocketAddr = "[2001:db8::7]:40000".parse().unwrap();

        let response = binding_response(&binding_request(id), address);

        assert_eq!(mapped_address(&response, id), Ok(address));
    }

    #[test]
    fn a_plain_mapped_address_is_read_when_no_xor_address_is_sent() {
        let mut response = RFC_5769_RESPONSE[..20].to_vec();
        response[3] = 12;
        response.extend_from_slice(&[0x00, 0x01, 0x00, 0x08, 0x00, 0x01, 0x0d, 0x96]);
        response.extend_from_slice(&[198, 51, 100, 9]);

        assert_eq!(
            mapped_address(&response, RFC_5769_ID),
            Ok("198.51.100.9:3478".parse().unwrap())
        );
    }

    #[test]
    fn foreign_or_broken_messages_are_refused() {
        let other = TransactionId([0; 12]);
        let mut error_response = RFC_5769_RESPONSE;
        error_response[1] = 0x11;
        let mut no_cookie = RFC_5769_RESPONSE;
        no_cookie[4] = 0;
        let mut no_attributes = RFC_5769_RESPONSE[..20].to_vec();
        no_attributes[3] = 0;

        assert_eq!(
            mapped_address(&RFC_5769_RESPONSE, other),
            Err(DecodeError::OtherTransaction)
        );
        assert_eq!(
            mapped_address(&error_response, RFC_5769_ID),
            Err(DecodeError::NotBindingSuccess {
                message_type: 0x0111
            })
        );
        assert_eq!(
            mapped_address(&no_cookie, RFC_5769_ID),
            Err(DecodeError::NotStun)
        );
        assert_eq!(
            mapped_address(&RFC_5769_RESPONSE[..50], RFC_5769_ID),
            Err(DecodeError::Truncated)
        );
        assert_eq!(
            mapped_address(&no_attributes, RFC_5769_ID),
            Err(DecodeError::NoMappedAddress)
        );
    }
}
