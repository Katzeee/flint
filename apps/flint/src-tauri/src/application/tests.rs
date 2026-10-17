use super::*;
use anyhow::Result;
use flint_backend::store::Store;
use flint_contracts::config::{Endpoints, StateDir};
use std::{net::TcpListener, path::Path};

fn application(directory: &Path) -> Result<Application> {
    let control = TcpListener::bind(("127.0.0.1", 0))?;
    let bridge = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(Application {
        config: Config {
            endpoints: Endpoints {
                control: control.local_addr()?,
                bridge: bridge.local_addr()?,
            },
            timeout: std::time::Duration::from_secs(5),
            state: StateDir::new(directory.into()),
        },
    })
}

#[tokio::test]
async fn execution_wait_outlives_control_deadline() -> Result<()> {
    use flint_contracts::protocol::{envelope, envelope::Payload, framed, read_envelope};
    use futures_util::SinkExt;
    use std::time::Duration;

    let directory = tempfile::tempdir()?;
    let mut api = application(directory.path())?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    api.config.endpoints.control = listener.local_addr()?;
    let (finish, released) = tokio::sync::oneshot::channel();
    let (received, request_received) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let mut wire = framed(listener.accept().await?.0);
        let request = read_envelope(&mut wire).await?;
        assert!(matches!(request.payload, Some(Payload::ExecuteRequest(_))));
        received.send(()).unwrap();
        released.await?;
        wire.send(envelope(
            request.request_id,
            Payload::ExecuteResponse(ExecuteResponse {
                execution_id: "0001".into(),
                status: ExecutionStatus::Succeeded as i32,
                ..Default::default()
            }),
        ))
        .await?;
        Ok::<_, anyhow::Error>(())
    });
    let waiting = tokio::spawn(async move { api.execute(ExecuteRequest::default()).await });
    request_received.await?;
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::task::yield_now().await;
    assert!(
        !waiting.is_finished(),
        "the control deadline must not end the execution's wait"
    );
    tokio::time::resume();
    finish.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), waiting).await??;
    assert_eq!(result?.status(), ExecutionStatus::Succeeded);
    peer.await??;
    Ok(())
}

#[test]
fn offline_queries_do_not_start_backend() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let api = application(directory.path())?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let snapshot = api.snapshot().await?;
        assert!(snapshot.backend.is_none());
        assert!(snapshot.apps.is_empty());
        assert!(api.apps(None).await.unwrap_err().is(FailureCode::BackendUnavailable));
        Ok::<_, anyhow::Error>(())
    })?;
    assert!(!directory.path().join("runtime").exists());
    assert!(!directory.path().join("workflows").exists());
    Ok(())
}

#[test]
fn snapshot_preserves_query_errors_when_backend_disappears() -> Result<()> {
    use flint_contracts::protocol::{envelope, envelope::Payload, framed, read_envelope};
    use futures_util::SinkExt;

    tokio::runtime::Runtime::new()?.block_on(async {
        let rejected = Failure::new(FailureCode::BackendStopping);
        for (reply, expected) in [
            (None, None),
            (Some(Payload::Failure(rejected.clone())), Some(rejected.code.clone())),
        ] {
            let directory = tempfile::tempdir()?;
            let mut api = application(directory.path())?;
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
            api.config.endpoints.control = listener.local_addr()?;
            let peer = async {
                let mut ping = framed(listener.accept().await?.0);
                let request = read_envelope(&mut ping).await?;
                assert!(matches!(request.payload, Some(Payload::PingRequest(_))));
                ping.send(envelope(
                    request.request_id,
                    Payload::PingResponse(PingResponse::default()),
                ))
                .await?;
                let mut query = framed(listener.accept().await?.0);
                let request = read_envelope(&mut query).await?;
                assert!(matches!(request.payload, Some(Payload::ListAppsRequest(_))));
                // The backend is gone before the query's outcome reaches the client.
                drop(listener);
                if let Some(reply) = reply {
                    query.send(envelope(request.request_id, reply)).await?;
                }
                Ok::<_, anyhow::Error>(())
            };
            let (peer, snapshot) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(peer, api.snapshot())
            })
            .await?;
            peer?;
            match (expected, snapshot) {
                (None, Ok(snapshot)) => {
                    assert!(snapshot.backend.is_none());
                    assert!(snapshot.apps.is_empty());
                }
                (Some(code), Err(failure)) => assert_eq!(failure.code, code),
                _ => panic!("snapshot must recover only a lost backend connection"),
            }
        }
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn workflow_queries_return_records_and_respect_execution_view() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path().join("workflows"))?;
    let workflow = store.create("API 场景".into(), "protocol queries".into())?;
    let code = "print('details')";
    let execution = store.append(&workflow, "query-host", code.into(), "capture".into())?;
    store.update(&workflow, &execution, |entry| {
        entry.set_status(ExecutionStatus::Succeeded);
    })?;
    drop(store);
    let api = application(directory.path())?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let backend = flint_backend::Backend::bind(api.config.clone()).await?;
        let server = tokio::spawn(backend.run());
        let full = api
            .execution(workflow.clone(), execution.clone(), ExecutionView::Full)
            .await?;
        assert_eq!(full.code.as_deref(), Some(code));
        let summary = api
            .execution(workflow.clone(), execution.clone(), ExecutionView::Summary)
            .await?;
        assert!(summary.code.is_none());
        assert_eq!(summary.execution_id, execution);
        let details = api.workflow(workflow).await?;
        assert_eq!(details.execs, vec![full]);
        api.stop_backend().await?;
        server.await??;
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(())
}
