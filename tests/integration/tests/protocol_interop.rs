mod support;
use anyhow::{Context, Result};
use bytes::BytesMut;
use flint_protocol::{envelope::Payload, *};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use support::*;
use tokio_util::codec::{Decoder, Encoder};

fn samples() -> Vec<Envelope> {
    let payloads = vec![
        Payload::PingRequest(PingRequest::default()),
        Payload::PingResponse(PingResponse {
            ready: true,
            pid: 42,
            backend_id: "backend".into(),
            registry_host: "127.0.0.1".into(),
            registry_port: 6321,
        }),
        Payload::StopBackendRequest(StopBackendRequest {
            backend_id: "backend".into(),
        }),
        Payload::StopBackendResponse(StopBackendResponse { stopping: true }),
        Payload::ShowWindowRequest(ShowWindowRequest::default()),
        Payload::ShowWindowResponse(ShowWindowResponse { accepted: true }),
        Payload::ListInstancesRequest(ListInstancesRequest {
            instance_type: Some("maya".into()),
        }),
        Payload::ListInstancesResponse(ListInstancesResponse {
            instances: vec![InstanceInfo {
                instance_id: "maya-1".into(),
                instance_name: "场景😀".into(),
                instance_type: "maya".into(),
                pid: 42,
                runtime_version: "CPython 3.7".into(),
                bridge_version: "0.1.0".into(),
                execution_ready: true,
            }],
        }),
        Payload::StartWorkflowRequest(StartWorkflowRequest {
            name: "work".into(),
            description: "描述".into(),
        }),
        Payload::StartWorkflowResponse(StartWorkflowResponse {
            workflow_id: "workflow".into(),
        }),
        Payload::ExecuteRequest(ExecuteRequest::default()),
        Payload::GetExecutionRequest(GetExecutionRequest {
            workflow_id: "workflow".into(),
            execution_id: "1".into(),
            view: ExecutionView::Full as i32,
        }),
        Payload::GetExecutionResponse(GetExecutionResponse {
            execution_id: "1".into(),
            workflow_id: "workflow".into(),
            status: ExecutionStatus::Succeeded as i32,
            stdout: "输出\n".into(),
            stderr: "错误\n".into(),
            code: Some(String::new()),
            ..Default::default()
        }),
        Payload::ProtocolError(ProtocolError {
            code: ErrorCode::InstanceBusy as i32,
            message: "busy".into(),
        }),
        Payload::RegisterInstance(RegisterInstance {
            pid: 42,
            name_hint: "maya".into(),
            instance_name: "场景".into(),
            instance_type: "maya".into(),
            bridge_id: "bridge".into(),
            runtime_version: "CPython 3.7".into(),
            bridge_version: "0.1.0".into(),
        }),
        Payload::RegisterExecutionChannel(RegisterExecutionChannel {
            instance_id: "maya-1".into(),
            pid: 42,
            session_token: "token".into(),
        }),
        Payload::Heartbeat(Heartbeat {
            instance_id: "maya-1".into(),
        }),
        Payload::InstanceAck(InstanceAck {
            success: true,
            error: None,
            message: String::new(),
            instance_id: "maya-1".into(),
            session_token: "token".into(),
        }),
        Payload::HostExecuteRequest(HostExecuteRequest {
            execution_id: "1".into(),
            code: "print('你好 🌍')\n".into(),
            workflow_id: "workflow".into(),
            execution_name: Some(String::new()),
            filename: None,
        }),
        Payload::ExecutionOutputUpdate(ExecutionOutputUpdate {
            execution_id: "1".into(),
            workflow_id: "workflow".into(),
            sequence: u64::MAX,
            stdout_delta: "输出\n".into(),
            stderr_delta: "错误\n".into(),
        }),
        Payload::ExecutionResult(ExecutionResult {
            execution_id: "1".into(),
            status: ExecutionStatus::Failed as i32,
            traceback: Some("traceback\n".into()),
            error: Some("宿主异常：任意文本".into()),
        }),
    ];
    let mut result: Vec<_> = payloads
        .into_iter()
        .enumerate()
        .map(|(i, p)| Envelope {
            protocol_version: 1,
            request_id: format!("case-{i}"),
            payload: Some(p),
        })
        .collect();
    for (i, filename) in [None, Some(String::new()), Some("测试.py".into())]
        .into_iter()
        .enumerate()
    {
        result.push(Envelope {
            protocol_version: 1,
            request_id: format!("optional-{i}"),
            payload: Some(Payload::ExecuteRequest(ExecuteRequest {
                instance_id: "maya-1".into(),
                workflow_id: "workflow".into(),
                code: "print('你好 🌍')".into(),
                filename,
                ..Default::default()
            })),
        });
    }
    result
}
fn write_samples(path: &Path, messages: &[Envelope]) -> Result<()> {
    let mut bytes = BytesMut::new();
    for message in messages {
        EnvelopeCodec::default().encode(message.clone(), &mut bytes)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}
fn read_samples(path: &Path) -> Result<Vec<Envelope>> {
    let mut bytes = BytesMut::from(fs::read(path)?.as_slice());
    let mut codec = EnvelopeCodec::default();
    let mut messages = vec![];
    while let Some(message) = codec.decode_eof(&mut bytes)? {
        messages.push(message);
    }
    Ok(messages)
}
fn echo_connection(mut stream: TcpStream) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    loop {
        let mut header = [0u8; 4];
        if stream.read(&mut header[..1])? == 0 {
            break;
        }
        stream.read_exact(&mut header[1..])?;
        let length = u32::from_be_bytes(header) as usize;
        anyhow::ensure!(length > 0 && length <= MAX_FRAME_BYTES, "Invalid length");
        let mut body = vec![0; length];
        stream.read_exact(&mut body)?;
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&body);
        let message = EnvelopeCodec::default()
            .decode(&mut frame)?
            .context("Missing envelope")?;
        let mut reply = BytesMut::new();
        EnvelopeCodec::default().encode(message, &mut reply)?;
        stream.write_all(&reply)?;
    }
    Ok(())
}
struct Echo {
    port: u16,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
}
impl Echo {
    fn start(connections: usize) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(60);
            let mut accepted = 0;
            while accepted < connections && !stop.load(Ordering::SeqCst) {
                anyhow::ensure!(Instant::now() < deadline, "Peer did not connect");
                match listener.accept() {
                    Ok((stream, _)) => {
                        echo_connection(stream)?;
                        accepted += 1;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(())
        });
        Ok(Self {
            port,
            cancel,
            thread: Some(thread),
        })
    }
    fn finish(mut self) -> Result<()> {
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for Echo {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn python_peer(bundle: &Path, port: u16, input: &Path, output: &Path) -> Result<()> {
    checked(
        Command::new(python())
            .args(["-I", "-S", "-X", "utf8"])
            .arg(fixture("protocol_peer.py"))
            .arg(bundle)
            .arg(port.to_string())
            .arg(input)
            .arg(output),
        Duration::from_secs(30),
    )?;
    Ok(())
}

#[test]
fn python_protocol_preserves_messages_through_rust_tcp() -> Result<()> {
    let app = App::new();
    let bundle = app.export()?;
    let messages = samples();
    let input = app.directory.join("input.bin");
    let output = app.directory.join("python.bin");
    write_samples(&input, &messages)?;
    let echo = Echo::start(1)?;
    python_peer(&bundle, echo.port, &input, &output)?;
    echo.finish()?;
    assert_eq!(read_samples(&output)?, messages);
    Ok(())
}

#[test]
#[ignore = "Requires the .NET SDK selected by bridges/dotnet/global.json"]
fn csharp_and_python_protocols_preserve_messages_through_rust_tcp() -> Result<()> {
    let app = App::new();
    let bundle = app.export()?;
    let dotnet = std::env::var_os("FLINT_DOTNET").unwrap_or_else(|| "dotnet".into());
    let project = root().join("bridges/dotnet");
    let output_dir = app.directory.join("csharp");
    checked(
        Command::new(&dotnet)
            .current_dir(&project)
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .args([
                "build",
                "tests/Protocol.Interop/Peer.csproj",
                "--configuration",
                "Release",
                "-p:RestoreLockedMode=true",
                "--output",
            ])
            .arg(&output_dir),
        Duration::from_secs(300),
    )?;
    let messages = samples();
    let input = app.directory.join("input.bin");
    let csharp = app.directory.join("csharp.bin");
    let output = app.directory.join("python.bin");
    write_samples(&input, &messages)?;
    let echo = Echo::start(2)?;
    checked(
        Command::new(dotnet)
            .current_dir(project)
            .arg(output_dir.join("Peer.dll"))
            .arg(echo.port.to_string())
            .arg(&input)
            .arg(&csharp),
        Duration::from_secs(30),
    )?;
    assert_eq!(read_samples(&csharp)?, messages);
    python_peer(&bundle, echo.port, &csharp, &output)?;
    echo.finish()?;
    assert_eq!(read_samples(&output)?, messages);
    Ok(())
}
