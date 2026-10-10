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
            endpoints: Endpoints::local(control.local_addr()?.port(), bridge.local_addr()?.port()),
            timeout: std::time::Duration::from_secs(5),
            state: StateDir::new(directory.into()),
        },
    })
}

#[test]
fn offline_queries_and_unsupported_attach_do_not_start_backend() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let api = application(directory.path())?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let snapshot = api.snapshot().await?;
        assert!(snapshot.backend.is_none());
        assert!(snapshot.instances.is_empty());
        assert!(
            api.instances(None)
                .await
                .unwrap_err()
                .is(FailureCode::BackendUnavailable)
        );
        assert_eq!(
            api.attach(1, Some(HostKind::StandaloneCsharp), None)
                .await
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
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
                assert!(matches!(request.payload, Some(Payload::ListInstancesRequest(_))));
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
                    assert!(snapshot.instances.is_empty());
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
        entry.status = ExecutionStatus::Succeeded;
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
