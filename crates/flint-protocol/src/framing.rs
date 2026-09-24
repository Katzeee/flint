use crate::Envelope;
use bytes::BytesMut;
use prost::Message;
use std::io;
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 100 * 1024 * 1024;

pub fn validate(envelope: &Envelope) -> io::Result<()> {
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if envelope.request_id.is_empty() {
        return Err(invalid("missing request_id"));
    }
    if envelope.payload.is_none() {
        return Err(invalid("missing or unsupported payload"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// A frame is a four-byte unsigned big-endian body length followed by one envelope.
/// Use with Tokio's Framed. A framing error requires closing the connection;
/// callers own deadlines, cancellation, and serialized writes.
pub struct EnvelopeCodec {
    framing: LengthDelimitedCodec,
    limit: usize,
    pending: bool,
}

impl Default for EnvelopeCodec {
    fn default() -> Self {
        Self::with_limit(MAX_FRAME_BYTES).expect("valid default frame limit")
    }
}

impl EnvelopeCodec {
    pub fn with_limit(limit: usize) -> io::Result<Self> {
        if limit == 0 || limit > MAX_FRAME_BYTES {
            return Err(invalid("invalid frame limit"));
        }
        Ok(Self {
            framing: LengthDelimitedCodec::builder()
                .big_endian()
                .length_field_length(4)
                .max_frame_length(limit)
                .new_codec(),
            limit,
            pending: false,
        })
    }
}

impl Decoder for EnvelopeCodec {
    type Item = Envelope;
    type Error = io::Error;

    fn decode(&mut self, source: &mut BytesMut) -> io::Result<Option<Envelope>> {
        self.pending |= !source.is_empty();
        let Some(bytes) = self.framing.decode(source)? else {
            return Ok(None);
        };
        self.pending = false;
        let envelope = Envelope::decode(bytes).map_err(|error| invalid(error.to_string()))?;
        validate(&envelope)?;
        Ok(Some(envelope))
    }

    fn decode_eof(&mut self, source: &mut BytesMut) -> io::Result<Option<Envelope>> {
        if let Some(envelope) = self.decode(source)? {
            return Ok(Some(envelope));
        }
        // LengthDelimitedCodec may have consumed a complete header already.
        // An empty buffer therefore does not by itself mean a clean EOF.
        if self.pending {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated frame",
            ));
        }
        Ok(None)
    }
}

impl Encoder<Envelope> for EnvelopeCodec {
    type Error = io::Error;

    fn encode(&mut self, envelope: Envelope, destination: &mut BytesMut) -> io::Result<()> {
        validate(&envelope)?;
        if envelope.encoded_len() > self.limit {
            return Err(invalid("frame exceeds limit"));
        }
        self.framing
            .encode(envelope.encode_to_vec().into(), destination)
    }
}

#[cfg(test)]
mod tests;
