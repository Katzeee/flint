use crate::protocol::{Envelope, EnvelopeCodec, PROTOCOL_VERSION, PingRequest, envelope::Payload};
use bytes::BytesMut;
use prost::Message;
use tokio_util::codec::{Decoder, Encoder};

fn sample() -> Envelope {
    Envelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: "request-1".into(),
        payload: Some(Payload::PingRequest(PingRequest {})),
    }
}

#[test]
fn decoding_rejects_invalid_envelope_fields() {
    let mut valid = BytesMut::new();
    EnvelopeCodec::default().encode(sample(), &mut valid).unwrap();
    assert_eq!(EnvelopeCodec::default().decode(&mut valid).unwrap(), Some(sample()));
    let mut version = sample();
    version.protocol_version = 1;
    let mut request = sample();
    request.request_id.clear();
    let mut body = sample();
    body.payload = None;
    for (case, item) in [
        ("old protocol version", version),
        ("missing request id", request),
        ("missing payload", body),
    ] {
        let payload = item.encode_to_vec();
        let mut bytes = BytesMut::from((payload.len() as u32).to_be_bytes().as_slice());
        bytes.extend_from_slice(&payload);
        assert_eq!(
            EnvelopeCodec::default().decode(&mut bytes).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData,
            "{case}: decode"
        );
    }
}
