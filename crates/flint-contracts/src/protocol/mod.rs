//! Language-neutral messages and length-prefixed transport framing.
pub mod failure;
pub mod framing;
pub use failure::FailureCode;
pub mod generated;
pub mod timing;
pub use framing::{
    envelope, first_message, framed, read_envelope, EnvelopeCodec, MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
};
pub use generated::*;
