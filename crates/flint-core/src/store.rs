use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

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
    pub status: String,
    pub stdout: String,
    pub stderr: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub traceback: Option<String>,
    pub error: Option<String>,
    pub updated_at: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Workflow {
    pub workflow_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub schema_version: u32,
    pub latest_execution_id: u64,
    pub execution_count: u64,
    pub instance_ids: Vec<String>,
    pub execs: Vec<Execution>,
}

pub struct Store {
    root: PathBuf,
    gate: Mutex<()>,
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let store = Self {
            root,
            gate: Mutex::new(()),
        };
        for item in fs::read_dir(&store.root)? {
            let path = item?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let mut workflow: Workflow = serde_json::from_slice(&fs::read(&path)?)
                .with_context(|| format!("Cannot read workflow {}", path.display()))?;
            let mut changed = false;
            for entry in &mut workflow.execs {
                if entry.status == "running" || entry.status == "pending" {
                    entry.status = "failed".into();
                    entry.error =
                        Some("Backend interrupted; host execution outcome is unknown".into());
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
    fn path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 180
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            bail!("invalid workflow id");
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    fn write(&self, path: &Path, workflow: &Workflow) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        file.write_all(&serde_json::to_vec_pretty(workflow)?)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
    pub fn load(&self, id: &str) -> Result<Workflow> {
        Ok(serde_json::from_slice(&fs::read(self.path(id)?)?)?)
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
                schema_version: 1,
                latest_execution_id: 0,
                execution_count: 0,
                instance_ids: vec![],
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
        request_id: String,
    ) -> Result<String> {
        let _guard = self.gate.lock().unwrap();
        let mut workflow = self.load(workflow_id)?;
        workflow.latest_execution_id += 1;
        workflow.execution_count += 1;
        let id = format!("{:04}", workflow.latest_execution_id);
        if !workflow.instance_ids.iter().any(|i| i == instance_id) {
            workflow.instance_ids.push(instance_id.into());
        }
        workflow.execs.push(Execution {
            execution_id: id.clone(),
            workflow_id: workflow_id.into(),
            instance_id: instance_id.into(),
            name,
            code,
            status: "running".into(),
            stdout: String::new(),
            stderr: String::new(),
            started_at: now(),
            finished_at: None,
            traceback: None,
            error: None,
            updated_at: None,
            request_id: Some(request_id),
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
            .context("execution not found")?;
        change(entry);
        entry.updated_at = Some(now());
        self.write(&self.path(workflow_id)?, &workflow)
    }
    pub fn execution(&self, workflow_id: &str, execution_id: &str) -> Result<Execution> {
        self.load(workflow_id)?
            .execs
            .into_iter()
            .find(|e| e.execution_id == execution_id)
            .context("execution not found")
    }
}

#[cfg(test)]
mod tests;
