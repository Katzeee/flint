use crate::protocol::{
    envelope::Payload, Envelope, EnvelopeCodec, ExecuteRequest, ExecutionResult, ExecutionStatus,
    Failure, GetExecutionResponse,
};
use bytes::BytesMut;
use prost::Message;
use tokio_util::codec::{Decoder, Encoder};

fn sample() -> Envelope {
    Envelope {
        protocol_version: 2,
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
fn rejects_invalid_envelope_fields() {
    let mut version = sample();
    version.protocol_version = 1;
    let mut request = sample();
    request.request_id.clear();
    let mut body = sample();
    body.payload = None;
    let mut failure = sample();
    failure.payload = Some(Payload::Failure(Failure {
        code: String::new(),
        message: "No machine-readable reason".into(),
    }));
    let mut execution = sample();
    execution.payload = Some(Payload::ExecutionResult(ExecutionResult {
        execution_id: "execution-1".into(),
        status: ExecutionStatus::Failed as i32,
        error: Some(Failure {
            code: " \t".into(),
            message: "Execution failed".into(),
        }),
        traceback: None,
    }));
    let mut lookup = sample();
    lookup.payload = Some(Payload::GetExecutionResponse(GetExecutionResponse {
        execution_id: "execution-1".into(),
        workflow_id: "workflow-1".into(),
        instance_id: "maya".into(),
        status: ExecutionStatus::Failed as i32,
        error: Some(Failure {
            code: String::new(),
            message: "Stored failure".into(),
        }),
        ..Default::default()
    }));
    for (case, item) in [
        ("old protocol version", version),
        ("missing request id", request),
        ("missing payload", body),
        ("empty failure code", failure),
        ("blank execution failure code", execution),
        ("empty stored failure code", lookup),
    ] {
        let payload = item.encode_to_vec();
        let mut bytes = BytesMut::from((payload.len() as u32).to_be_bytes().as_slice());
        bytes.extend_from_slice(&payload);
        assert_eq!(
            EnvelopeCodec::default()
                .decode(&mut bytes)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData,
            "{case}: decode"
        );
        assert_eq!(
            EnvelopeCodec::default()
                .encode(item, &mut BytesMut::new())
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData,
            "{case}: encode"
        );
    }
}
