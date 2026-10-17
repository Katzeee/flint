use super::*;
use flint_contracts::config::{Endpoints, StateDir};

async fn connect(backend: &BackendHandle, request: Payload) -> (Wire, tokio::task::JoinHandle<Result<()>>, Payload) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut wire = framed(TcpStream::connect(listener.local_addr().unwrap()).await.unwrap());
    let socket = listener.accept().await.unwrap().0;
    let backend = backend.clone();
    let task = tokio::spawn(async move { connection(backend, socket).await });
    wire.send(envelope("register".into(), request)).await.unwrap();
    let response = read_envelope(&mut wire).await.unwrap();
    (wire, task, response.payload.unwrap())
}

fn registration(id: &str) -> Payload {
    Payload::RegisterInstance(RegisterInstance {
        pid: 42,
        instance_id: id.into(),
        instance_name: "Editor".into(),
        instance_type: "unity".into(),
        token: flint_contracts::config::bridge_token::load_or_create().unwrap(),
        ..Default::default()
    })
}

async fn register(backend: &BackendHandle, id: &str) -> (Wire, tokio::task::JoinHandle<Result<()>>, String) {
    let (wire, task, response) = connect(backend, registration(id)).await;
    let Payload::SessionRegistered(registered) = response else {
        panic!("registration refused: {response:?}");
    };
    (wire, task, registered.session_id)
}

async fn channel(backend: &BackendHandle, session_id: &str) -> (Wire, tokio::task::JoinHandle<Result<()>>) {
    let (wire, task, response) = connect(
        backend,
        Payload::RegisterExecutionChannel(RegisterExecutionChannel {
            session_id: session_id.into(),
        }),
    )
    .await;
    assert!(matches!(response, Payload::Ack(_)));
    (wire, task)
}

fn execute(
    backend: &BackendHandle,
    id: &str,
    workflow: &str,
) -> tokio::task::JoinHandle<std::result::Result<ExecutionResult, Failure>> {
    let backend = backend.clone();
    let request = ExecuteRequest {
        instance_id: id.into(),
        workflow_id: workflow.into(),
        code: "return 1;".into(),
        ..Default::default()
    };
    tokio::spawn(async move {
        let (_, completion) = control::execute(&backend, request)?;
        Ok(completion.await.unwrap())
    })
}

#[tokio::test]
async fn stable_instances_share_pids_and_replacement_isolates_old_sessions_and_jobs() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let directory = tempfile::tempdir().unwrap();
        let backend = BackendHandle(Arc::new(Shared {
            config: Config {
                endpoints: Endpoints::default(),
                timeout: Duration::from_secs(5),
                state: StateDir::new(directory.path().into()),
            },
            state: Mutex::new(State::default()),
            store: Store::open(directory.path().into()).unwrap(),
            shutdown: CancellationToken::new(),
        }));
        let workflow = backend.0.store.create("reload".into(), String::new()).unwrap();
        let id = "abcdefghijklmnop";
        let (_old_heartbeat, old_task, old_session) = register(&backend, id).await;
        let (_remote, remote_task, remote_session) = register(&backend, "ABCDEFGHIJKLMNOP").await;
        assert_eq!(
            backend.instances().len(),
            2,
            "equal PIDs on distinct machines must coexist"
        );
        let (mut old_channel, old_channel_task) = channel(&backend, &old_session).await;
        let interrupted = execute(&backend, id, &workflow);
        let old_request = read_envelope(&mut old_channel).await.unwrap();
        assert!(matches!(old_request.payload, Some(Payload::HostExecuteRequest(_))));

        let (_new_heartbeat, new_task, new_session) = register(&backend, id).await;
        assert_ne!(new_session, old_session);
        let failed = interrupted.await.unwrap().unwrap();
        assert_eq!(failed.error.unwrap().code, "execution_disconnected");
        let _ = old_task.await.unwrap();
        let _ = old_channel_task.await.unwrap();
        // Exercise delayed cleanup even if cancellation won the scheduling race.
        disconnect(&backend, &old_session);
        assert_eq!(backend.instances().len(), 2);

        let (mut current, current_task) = channel(&backend, &new_session).await;
        let next = execute(&backend, id, &workflow);
        let request = read_envelope(&mut current).await.unwrap();
        let Some(Payload::HostExecuteRequest(command)) = request.payload else {
            panic!("missing new request");
        };
        let result = envelope(
            request.request_id.clone(),
            Payload::ExecutionResult(ExecutionResult {
                execution_id: command.execution_id,
                status: ExecutionStatus::Succeeded as i32,
                ..Default::default()
            }),
        );
        current.send(result).await.unwrap();
        assert_eq!(next.await.unwrap().unwrap().status(), ExecutionStatus::Succeeded);
        disconnect(&backend, &new_session);
        disconnect(&backend, &remote_session);
        let _ = new_task.await.unwrap();
        let _ = current_task.await.unwrap();
        let _ = remote_task.await.unwrap();
    })
    .await
    .unwrap();
}

#[test]
fn only_the_exact_nonempty_token_matches() {
    assert!(token_matches("secret", "secret"));
    assert!(!token_matches("secret", "secreT"));
    assert!(!token_matches("secret", "secret2"));
    assert!(!token_matches("", ""));
}
