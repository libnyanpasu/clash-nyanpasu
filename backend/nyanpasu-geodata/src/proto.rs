//! Bounds-checked protobuf field reader that borrows from its input.
use crate::{GeoError, GeoResult};

pub(crate) enum Value<'a> {
    Varint(u64),
    /// Length-delimited and fixed-width payloads.
    Bytes(&'a [u8]),
}

impl<'a> Value<'a> {
    pub(crate) fn bytes(self) -> GeoResult<&'a [u8]> {
        match self {
            Value::Bytes(bytes) => Ok(bytes),
            Value::Varint(_) => Err(GeoError::Malformed(
                "protobuf field is not length-delimited",
            )),
        }
    }

    pub(crate) fn varint(self) -> GeoResult<u64> {
        match self {
            Value::Varint(value) => Ok(value),
            Value::Bytes(_) => Err(GeoError::Malformed("protobuf field is not a varint")),
        }
    }
}

/// The `(field number, value)` pairs of one message; stops after an error.
pub(crate) struct Fields<'a> {
    buf: &'a [u8],
}

impl<'a> Fields<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn field(&mut self) -> GeoResult<(u64, Value<'a>)> {
        let key = self.varint()?;
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => Value::Bytes(self.take(8)?),
            2 => {
                let len = usize::try_from(self.varint()?).unwrap_or(usize::MAX);
                Value::Bytes(self.take(len)?)
            }
            5 => Value::Bytes(self.take(4)?),
            _ => return Err(GeoError::Malformed("unsupported protobuf wire type")),
        };
        Ok((key >> 3, value))
    }

    fn varint(&mut self) -> GeoResult<u64> {
        let mut value = 0;
        for (i, byte) in self.buf.iter().take(10).enumerate() {
            value |= u64::from(byte & 0x7f) << (7 * i);
            if *byte < 0x80 {
                self.buf = &self.buf[i + 1..];
                return Ok(value);
            }
        }
        Err(GeoError::Malformed("truncated or overlong protobuf varint"))
    }

    fn take(&mut self, len: usize) -> GeoResult<&'a [u8]> {
        if len > self.buf.len() {
            return Err(GeoError::Malformed("protobuf field runs past its message"));
        }
        let (head, tail) = self.buf.split_at(len);
        self.buf = tail;
        Ok(head)
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = GeoResult<(u64, Value<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() {
            return None;
        }
        let field = self.field();
        if field.is_err() {
            self.buf = &[];
        }
        Some(field)
    }
}
