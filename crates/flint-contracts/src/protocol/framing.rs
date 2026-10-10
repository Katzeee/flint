use crate::protocol::{Envelope, envelope::Payload};
use bytes::BytesMut;
use futures_util::StreamExt;
use prost::Message;
use std::{io, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, Framed, LengthDelimitedCodec};

pub const PROTOCOL_VERSION: u32 = 4;
pub const MAX_FRAME_BYTES: usize = 100 * 1024 * 1024;

pub type Wire<T> = Framed<T, EnvelopeCodec>;

pub fn framed<T>(stream: T) -> Wire<T> {
    Framed::new(stream, EnvelopeCodec::default())
}

/// Opens a framed stream and reads its first message within the caller's deadline.
/// Keep the returned wire to preserve any subsequent messages already buffered.
pub async fn first_message<T: AsyncRead + AsyncWrite + Unpin>(
    stream: T,
    timeout: Duration,
) -> io::Result<(Wire<T>, Envelope)> {
    let mut wire = framed(stream);
    let first = tokio::time::timeout(timeout, read_envelope(&mut wire))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "first message timed out"))??;
    Ok((wire, first))
}

pub fn envelope(request_id: String, payload: Payload) -> Envelope {
    Envelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        payload: Some(payload),
    }
}

/// Reads one required message. The caller owns deadlines and cancellation.
pub async fn read_envelope<T: AsyncRead + AsyncWrite + Unpin>(wire: &mut Wire<T>) -> io::Result<Envelope> {
    wire.next().await.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed before a message arrived",
        )
    })?
}

fn validate(envelope: &Envelope) -> io::Result<()> {
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
    pending: bool,
}

impl Default for EnvelopeCodec {
    fn default() -> Self {
        Self {
            framing: LengthDelimitedCodec::builder()
                .big_endian()
                .length_field_length(4)
                .max_frame_length(MAX_FRAME_BYTES)
                .new_codec(),
            pending: false,
        }
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
        let envelope = Envelope::decode(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
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
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated frame"));
        }
        Ok(None)
    }
}

impl Encoder<Envelope> for EnvelopeCodec {
    type Error = io::Error;

    fn encode(&mut self, envelope: Envelope, destination: &mut BytesMut) -> io::Result<()> {
        if envelope.encoded_len() > MAX_FRAME_BYTES {
            return Err(invalid("frame exceeds limit"));
        }
        self.framing.encode(envelope.encode_to_vec().into(), destination)
    }
}

#[cfg(test)]
mod tests;
