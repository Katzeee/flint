use bytes::BytesMut;
use flint_protocol::{envelope::Payload, Envelope, EnvelopeCodec, ExecuteRequest, MAX_FRAME_BYTES};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use tokio_util::codec::{Decoder, Encoder, Framed};

fn sample() -> Envelope {
    Envelope {
        protocol_version: 1,
        request_id: "request-1".into(),
        payload: Some(Payload::ExecuteRequest(ExecuteRequest {
            instance_id: "maya".into(),
            code: "print('你好 🌍')\n".into(),
            filename: Some(String::new()),
            ..Default::default()
        })),
    }
}

fn framed(envelope: Envelope) -> BytesMut {
    let mut bytes = BytesMut::new();
    EnvelopeCodec::default()
        .encode(envelope, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn fragmented_and_coalesced_frames_preserve_messages() {
    let first = sample();
    let mut second = sample();
    second.request_id = "request-2".into();
    let mut wire = framed(first.clone());
    wire.extend_from_slice(&framed(second.clone()));
    let mut codec = EnvelopeCodec::default();
    let mut buffer = BytesMut::new();
    let mut received = Vec::new();
    for byte in wire {
        buffer.extend_from_slice(&[byte]);
        while let Some(item) = codec.decode(&mut buffer).unwrap() {
            received.push(item);
        }
    }
    assert_eq!(received, vec![first, second]);
    assert!(codec.decode_eof(&mut buffer).unwrap().is_none());
}

#[test]
fn rejects_truncated_header_and_body_at_eof() {
    let frame = framed(sample());
    for length in 1..frame.len() {
        let mut codec = EnvelopeCodec::default();
        let mut input = BytesMut::from(&frame[..length]);
        assert!(codec.decode(&mut input).unwrap().is_none());
        assert!(
            codec.decode_eof(&mut input).is_err(),
            "accepted truncation at {length}"
        );
    }
}

#[test]
fn rejects_invalid_lengths_before_reading_a_body() {
    for length in [0, MAX_FRAME_BYTES as u32 + 1, u32::MAX] {
        let mut bytes = BytesMut::from(length.to_be_bytes().as_slice());
        assert!(EnvelopeCodec::default().decode(&mut bytes).is_err());
    }
    let mut bytes = BytesMut::new();
    assert!(EnvelopeCodec::with_limit(8)
        .unwrap()
        .encode(sample(), &mut bytes)
        .is_err());
    assert!(bytes.is_empty());
}

#[test]
fn rejects_invalid_protobuf_and_envelopes() {
    let mut malformed = BytesMut::from(&b"\0\0\0\x01\xff"[..]);
    assert!(EnvelopeCodec::default().decode(&mut malformed).is_err());
    let mut version = sample();
    version.protocol_version = 2;
    let mut request = sample();
    request.request_id.clear();
    let mut body = sample();
    body.payload = None;
    for item in [version, request, body] {
        let payload = item.encode_to_vec();
        let mut bytes = BytesMut::from((payload.len() as u32).to_be_bytes().as_slice());
        bytes.extend_from_slice(&payload);
        assert!(EnvelopeCodec::default().decode(&mut bytes).is_err());
        assert!(EnvelopeCodec::default()
            .encode(item, &mut BytesMut::new())
            .is_err());
    }
}

#[test]
fn optional_empty_and_absent_are_distinct() {
    let empty = sample();
    let mut absent = empty.clone();
    if let Some(Payload::ExecuteRequest(request)) = &mut absent.payload {
        request.filename = None;
    }
    assert_ne!(empty.encode_to_vec(), absent.encode_to_vec());
    for item in [empty, absent] {
        let decoded = EnvelopeCodec::default()
            .decode(&mut framed(item.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(decoded, item);
    }
}

#[tokio::test]
async fn tcp_connection_carries_multiple_correlated_messages() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut connection = Framed::new(socket, EnvelopeCodec::default());
        while let Some(message) = connection.next().await {
            connection.send(message.unwrap()).await.unwrap();
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let mut connection = Framed::new(socket, EnvelopeCodec::default());
        for i in 0..3 {
            let mut message = sample();
            message.request_id = i.to_string();
            connection.send(message).await.unwrap();
        }
        for i in 0..3 {
            assert_eq!(
                connection.next().await.unwrap().unwrap().request_id,
                i.to_string()
            );
        }
        drop(connection);
        server.await.unwrap();
    })
    .await
    .unwrap();
}
