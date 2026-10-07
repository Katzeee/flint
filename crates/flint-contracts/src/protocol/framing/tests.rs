use crate::protocol::{
    envelope::Payload, Envelope, EnvelopeCodec, ExecuteRequest, MAX_FRAME_BYTES,
};
use bytes::BytesMut;
use futures_util::SinkExt;
use prost::Message;
use tokio_util::codec::{Decoder, Encoder};

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

#[tokio::test]
async fn tcp_connection_carries_multiple_correlated_messages() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (mut connection, first) =
            crate::protocol::first_message(socket, std::time::Duration::from_secs(5))
                .await
                .unwrap();
        connection.send(first).await.unwrap();
        for _ in 1..3 {
            let message = crate::protocol::read_envelope(&mut connection)
                .await
                .unwrap();
            connection.send(message).await.unwrap();
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let mut connection = crate::protocol::framed(socket);
        for i in 0..3 {
            let mut message = sample();
            message.request_id = i.to_string();
            connection.send(message).await.unwrap();
        }
        for i in 0..3 {
            assert_eq!(
                crate::protocol::read_envelope(&mut connection)
                    .await
                    .unwrap()
                    .request_id,
                i.to_string()
            );
        }
        assert_eq!(
            crate::protocol::read_envelope(&mut connection)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );
        drop(connection);
        server.await.unwrap();
    })
    .await
    .unwrap();
}
