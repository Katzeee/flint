use flint_contracts::protocol::{Envelope, ExecuteResponse, RegisterInstance};
use std::{collections::HashMap, time::Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) struct Session {
    pub(super) registration: RegisterInstance,
    pub(super) cancel: CancellationToken,
    pub(super) sender: Option<mpsc::Sender<Envelope>>,
    pub(super) heartbeat: Instant,
}
pub(super) struct Job {
    pub(super) session_id: String,
    pub(super) workflow: String,
    pub(super) execution: String,
    pub(super) result: oneshot::Sender<ExecuteResponse>,
}
#[derive(Default)]
pub(super) struct State {
    pub(super) sessions: HashMap<String, Session>,
    pub(super) jobs: HashMap<String, Job>,
    pub(super) stopping: bool,
}
impl State {
    pub(super) fn session_for_instance(&self, instance_id: &str) -> Option<(&String, &Session)> {
        self.sessions
            .iter()
            .find(|(_, session)| session.registration.instance_id == instance_id)
    }

    /// Retire the previous session and its jobs before publishing its replacement.
    pub(super) fn register(&mut self, registration: RegisterInstance) -> (String, CancellationToken, Vec<Job>) {
        let previous = self
            .session_for_instance(&registration.instance_id)
            .map(|(id, _)| id.clone());
        let jobs = previous.map(|id| self.remove_session(&id)).unwrap_or_default();
        let session_id = Uuid::new_v4().simple().to_string();
        let cancel = CancellationToken::new();
        self.sessions.insert(
            session_id.clone(),
            Session {
                registration,
                cancel: cancel.clone(),
                sender: None,
                heartbeat: Instant::now(),
            },
        );
        (session_id, cancel, jobs)
    }

    pub(super) fn remove_session(&mut self, session_id: &str) -> Vec<Job> {
        let Some(session) = self.sessions.remove(session_id) else {
            return Vec::new();
        };
        session.cancel.cancel();
        let requests: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        requests.into_iter().filter_map(|id| self.jobs.remove(&id)).collect()
    }
}
