//! Language-neutral messages and length-prefixed transport framing.
pub mod framing;
pub mod generated;
pub use framing::{EnvelopeCodec, MAX_FRAME_BYTES, PROTOCOL_VERSION};
pub use generated::*;
