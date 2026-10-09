use anyhow::Context;
use chrono::Utc;
use flint_contracts::protocol::{ExecutionStatus, Failure, FailureCode, WorkflowSummary};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("invalid workflow id {0:?}")]
    InvalidWorkflowId(String),
    #[error("workflow {0} does not exist")]
    WorkflowNotFound(String),
    #[error("execution {execution_id} does not exist in workflow {workflow_id}")]
    ExecutionNotFound {
        workflow_id: String,
        execution_id: String,
    },
    #[error("cannot read workflow {}", .path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        let code = match error {
            StoreError::InvalidWorkflowId(_) => FailureCode::InvalidArguments,
            StoreError::WorkflowNotFound(_) => FailureCode::WorkflowNotFound,
            StoreError::ExecutionNotFound { .. } => FailureCode::ExecutionNotFound,
            StoreError::Unreadable { .. } => FailureCode::WorkflowUnreadable,
            StoreError::Storage(_) => FailureCode::InternalError,
        };
        Failure::caused_by(code, &error)
    }
}

type Result<T> = std::result::Result<T, StoreError>;

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Execution {
    pub execution_id: String,
    pub name: String,
    pub workflow_id: String,
    pub instance_id: String,
    pub code: String,
    pub status: ExecutionStatus,
    pub stdout: String,
    pub stderr: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub traceback: Option<String>,
    pub error: Option<Failure>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Workflow {
    pub workflow_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub schema_version: u32,
    pub latest_execution_id: u64,
    pub execs: Vec<Execution>,
}

/// Reads the version alone first, so an older record reports its version rather than a shape mismatch.
fn decode(path: &Path, bytes: io::Result<Vec<u8>>) -> Result<Workflow> {
    #[derive(Deserialize)]
    struct Header {
        schema_version: u32,
    }
    let decode = || -> anyhow::Result<Workflow> {
        let bytes = bytes?;
        let Header { schema_version } = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            schema_version == SCHEMA_VERSION,
            "unsupported schema version {schema_version}"
        );
        let workflow: Workflow = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            workflow
                .execs
                .iter()
                .all(|entry| entry.status != ExecutionStatus::Unspecified),
            "a recorded execution has no status"
        );
        anyhow::ensure!(
            workflow
                .execs
                .iter()
                .filter_map(|entry| entry.error.as_ref())
                .all(|failure| !failure.code.trim().is_empty()),
            "a recorded failure has no code"
        );
        Ok(workflow)
    };
    decode().map_err(|source| StoreError::Unreadable {
        path: path.into(),
        source,
    })
}

fn is_record(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("json")
}

pub struct Store {
    root: PathBuf,
    gate: Mutex<()>,
}
impl Store {
    /// Marks executions interrupted by the previous backend. Unreadable records stay
    /// untouched and fail only the operations that need them.
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("cannot create workflow directory {}", root.display()))?;
        let store = Self {
            root,
            gate: Mutex::new(()),
        };
        for path in store.records()? {
            let mut workflow = match decode(&path, fs::read(&path)) {
                Ok(workflow) => workflow,
                Err(error) => {
                    eprintln!("{:#}", anyhow::Error::from(error));
                    continue;
                }
            };
            let mut changed = false;
            for entry in &mut workflow.execs {
                if matches!(
                    entry.status,
                    ExecutionStatus::Pending | ExecutionStatus::Running
                ) {
                    entry.status = ExecutionStatus::Failed;
                    entry.error = Some(Failure::new(FailureCode::ExecutionInterrupted));
                    entry.finished_at = Some(now());
                    entry.updated_at = entry.finished_at.clone();
                    changed = true;
                }
            }
            if changed {
                store.write(&path, &workflow)?;
            }
        }
        Ok(store)
    }
    fn records(&self) -> Result<Vec<PathBuf>> {
        let list = || -> io::Result<Vec<PathBuf>> {
            fs::read_dir(&self.root)?
                .map(|item| Ok(item?.path()))
                .filter(|path| path.as_ref().map_or(true, |path| is_record(path)))
                .collect()
        };
        Ok(list().with_context(|| format!("cannot list workflows in {}", self.root.display()))?)
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 180
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(StoreError::InvalidWorkflowId(id.into()));
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    fn write(&self, path: &Path, workflow: &Workflow) -> Result<()> {
        let write = || -> anyhow::Result<()> {
            let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
            file.write_all(&serde_json::to_vec_pretty(workflow)?)?;
            file.as_file().sync_all()?;
            file.persist(path)?;
            Ok(())
        };
        Ok(write().with_context(|| format!("cannot write workflow {}", path.display()))?)
    }
    pub fn load(&self, id: &str) -> Result<Workflow> {
        let path = self.path(id)?;
        match fs::read(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(StoreError::WorkflowNotFound(id.into()))
            }
            bytes => decode(&path, bytes),
        }
    }
    /// Lists readable workflows; an unreadable record is reported when it is opened.
    pub fn list(&self) -> Result<Vec<WorkflowSummary>> {
        let _guard = self.gate.lock().unwrap();
        let mut summaries = Vec::new();
        for path in self.records()? {
            let Ok(workflow) = decode(&path, fs::read(&path)) else {
                continue;
            };
            let updated_at = workflow
                .execs
                .iter()
                .flat_map(|entry| {
                    [
                        Some(entry.started_at.as_str()),
                        entry.updated_at.as_deref(),
                        entry.finished_at.as_deref(),
                    ]
                })
                .flatten()
                .chain(std::iter::once(workflow.created_at.as_str()))
                .max()
                .unwrap()
                .to_owned();
            let mut instance_ids = Vec::new();
            for execution in &workflow.execs {
                if !instance_ids.contains(&execution.instance_id) {
                    instance_ids.push(execution.instance_id.clone());
                }
            }
            summaries.push(WorkflowSummary {
                workflow_id: workflow.workflow_id,
                name: workflow.name,
                description: workflow.description,
                updated_at,
                execution_count: workflow.execs.len() as u64,
                instance_ids,
                running_count: workflow
                    .execs
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.status,
                            ExecutionStatus::Pending | ExecutionStatus::Running
                        )
                    })
                    .count() as u64,
                failed_count: workflow
                    .execs
                    .iter()
                    .filter(|e| e.status == ExecutionStatus::Failed)
                    .count() as u64,
            });
        }
        summaries.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.workflow_id.cmp(&b.workflow_id))
        });
        Ok(summaries)
    }
    pub fn create(&self, name: String, description: String) -> Result<String> {
        let _guard = self.gate.lock().unwrap();
        let id = Uuid::new_v4().simple().to_string();
        self.write(
            &self.path(&id)?,
            &Workflow {
                workflow_id: id.clone(),
                name,
                description,
                created_at: now(),
                schema_version: SCHEMA_VERSION,
                latest_execution_id: 0,
                execs: vec![],
            },
        )?;
        Ok(id)
    }
    pub fn append(
        &self,
        workflow_id: &str,
        instance_id: &str,
        code: String,
        name: String,
    ) -> Result<String> {
        let _guard = self.gate.lock().unwrap();
        let mut workflow = self.load(workflow_id)?;
        workflow.latest_execution_id += 1;
        let id = format!("{:04}", workflow.latest_execution_id);
        workflow.execs.push(Execution {
            execution_id: id.clone(),
            workflow_id: workflow_id.into(),
            instance_id: instance_id.into(),
            name,
            code,
            status: ExecutionStatus::Running,
            stdout: String::new(),
            stderr: String::new(),
            started_at: now(),
            finished_at: None,
            traceback: None,
            error: None,
            updated_at: None,
        });
        self.write(&self.path(workflow_id)?, &workflow)?;
        Ok(id)
    }
    pub fn update(
        &self,
        workflow_id: &str,
        execution_id: &str,
        change: impl FnOnce(&mut Execution),
    ) -> Result<()> {
        let _guard = self.gate.lock().unwrap();
        let mut workflow = self.load(workflow_id)?;
        let entry = workflow
            .execs
            .iter_mut()
            .find(|e| e.execution_id == execution_id)
            .ok_or_else(|| missing_execution(workflow_id, execution_id))?;
        change(entry);
        entry.updated_at = Some(now());
        self.write(&self.path(workflow_id)?, &workflow)
    }
    pub fn execution(&self, workflow_id: &str, execution_id: &str) -> Result<Execution> {
        self.load(workflow_id)?
            .execs
            .into_iter()
            .find(|e| e.execution_id == execution_id)
            .ok_or_else(|| missing_execution(workflow_id, execution_id))
    }
}

fn missing_execution(workflow_id: &str, execution_id: &str) -> StoreError {
    StoreError::ExecutionNotFound {
        workflow_id: workflow_id.into(),
        execution_id: execution_id.into(),
    }
}

#[cfg(test)]
mod tests;
