#![allow(dead_code)]
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::{NamedTempFile, TempDir};
use wait_timeout::ChildExt;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}
pub fn binary() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_flint"));
    assert!(
        path.is_file(),
        "Cargo did not build flint: {}",
        path.display()
    );
    path
}
pub fn python() -> PathBuf {
    if let Some(path) = env::var_os("FLINT_TEST_PYTHON") {
        return PathBuf::from(path);
    }
    let output = Command::new("uv")
        .args(["python", "find", "3.13"])
        .output()
        .expect("uv is required to locate the test Python interpreter");
    assert!(output.status.success(), "uv could not find Python 3.13");
    let path = PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("Invalid Python path")
            .trim(),
    );
    assert!(
        path.is_file(),
        "Python interpreter does not exist: {}",
        path.display()
    );
    path
}
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}
pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}
pub struct OwnedProcess(pub Child);
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
pub fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
}
pub fn run(command: &mut Command, timeout: Duration, input: Option<&str>) -> Result<Output> {
    // File-backed capture cannot deadlock on full pipes or inherited EOF handles.
    let stdout = NamedTempFile::new()?;
    let stderr = NamedTempFile::new()?;
    command.stdout(stdout.reopen()?).stderr(stderr.reopen()?);
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    hidden(command);
    let description = format!("{command:?}");
    let mut process = OwnedProcess(command.spawn().with_context(|| description.clone())?);
    if let Some(input) = input {
        process
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())?;
    }
    let status = process.0.wait_timeout(timeout)?.with_context(|| {
        format!(
            "Timed out: {description}\n{}",
            fs::read_to_string(stderr.path()).unwrap_or_default()
        )
    })?;
    Ok(Output {
        status,
        stdout: String::from_utf8_lossy(&fs::read(stdout.path())?).into_owned(),
        stderr: String::from_utf8_lossy(&fs::read(stderr.path())?).into_owned(),
    })
}
pub fn checked(command: &mut Command, timeout: Duration) -> Result<Output> {
    let output = run(command, timeout, None)?;
    anyhow::ensure!(
        output.status.success(),
        "{}\n{}",
        output.stdout,
        output.stderr
    );
    Ok(output)
}
pub fn wait_until(timeout: Duration, mut condition: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if condition()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("Condition did not become true within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub struct App {
    pub directory: PathBuf,
    pub port: u16,
    pub registry_port: u16,
    pub binary: PathBuf,
    _temporary: Option<TempDir>,
}
impl App {
    pub fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let mut app = Self::at(temporary.path().to_path_buf());
        app._temporary = Some(temporary);
        app
    }
    pub fn evidence(name: &str) -> Self {
        let parent = root().join("target/integration-artifacts");
        fs::create_dir_all(&parent).unwrap();
        let path = tempfile::Builder::new()
            .prefix(&format!("{name}-"))
            .tempdir_in(parent)
            .unwrap()
            .keep();
        println!("EVIDENCE {}", path.display());
        Self::at(path)
    }
    fn at(directory: PathBuf) -> Self {
        let port = free_port();
        let mut registry_port = free_port();
        while port == registry_port {
            registry_port = free_port();
        }
        Self {
            directory,
            port,
            registry_port,
            binary: binary(),
            _temporary: None,
        }
    }
    pub fn command(&self, name: &str) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .current_dir(&self.directory)
            .env("FLINT_STATE_DIR", self.directory.join("state"))
            .args([
                name,
                "--host",
                "127.0.0.1",
                "--port",
                &self.port.to_string(),
                "--registry-host",
                "127.0.0.1",
                "--registry-port",
                &self.registry_port.to_string(),
                "--json",
                "--no-tray",
            ]);
        command
    }
    pub fn call(&self, name: &str, args: &[&str], expected: i32) -> Result<Value> {
        self.input(name, args, expected, None)
    }
    pub fn input(
        &self,
        name: &str,
        args: &[&str],
        expected: i32,
        input: Option<&str>,
    ) -> Result<Value> {
        let result = run(
            self.command(name).args(args),
            Duration::from_secs(45),
            input,
        )?;
        anyhow::ensure!(
            result.status.code() == Some(expected),
            "{name}: expected exit {expected}, got {}\n{}\n{}",
            result.status,
            result.stdout,
            result.stderr
        );
        anyhow::ensure!(result.stderr.is_empty(), "{name}: {}", result.stderr);
        Ok(serde_json::from_str(&result.stdout)
            .with_context(|| format!("Invalid JSON: {}", result.stdout))?)
    }
    pub fn export(&self) -> Result<PathBuf> {
        let bundle = self.directory.join("flint-python.zip");
        checked(
            Command::new(&self.binary)
                .current_dir(&self.directory)
                .args(["bridge", "export", "python"]),
            Duration::from_secs(15),
        )?;
        Ok(bundle)
    }
    pub fn export_unity(&self) -> Result<PathBuf> {
        let bundle = self.directory.join("flint-unity.tgz");
        checked(
            Command::new(&self.binary)
                .current_dir(&self.directory)
                .args(["bridge", "export", "unity"]),
            Duration::from_secs(15),
        )?;
        Ok(bundle)
    }
    pub fn workflow(&self, name: &str) -> Result<String> {
        Ok(self.call("workflow", &["--name", name], 0)?["workflow_id"]
            .as_str()
            .unwrap()
            .into())
    }
    pub fn execute(
        &self,
        instance: &str,
        workflow: &str,
        code: &str,
        expected: i32,
    ) -> Result<Value> {
        self.call(
            "exec",
            &[
                "--instance-id",
                instance,
                "--workflow-id",
                workflow,
                "--code",
                code,
            ],
            expected,
        )
    }
    pub fn details(&self, workflow: &str, execution: &Value, expected: i32) -> Result<Value> {
        self.call(
            "execution",
            &[
                "--workflow-id",
                workflow,
                "--execution-id",
                execution["execution_id"].as_str().unwrap(),
                "--view",
                "full",
            ],
            expected,
        )
    }
    pub fn instance(&self, host: &str) -> Result<Option<Value>> {
        let response = self.call("instances", &["--type", host], 0)?;
        Ok(response["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["execution_ready"] == true)
            .cloned())
    }
    pub fn await_instance(&self, host: &str, previous: Option<&str>) -> Result<Value> {
        let mut instance = None;
        wait_until(Duration::from_secs(30), || {
            instance = self
                .instance(host)?
                .filter(|i| Some(i["instance_id"].as_str().unwrap()) != previous);
            Ok(instance.is_some())
        })?;
        Ok(instance.unwrap())
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let output = run(
                self.command("stop").args(["--timeout", "5"]),
                Duration::from_secs(7),
                None,
            );
            if output.as_ref().is_ok_and(|o| o.status.success()) {
                break;
            }
            if Instant::now() >= deadline {
                let message = format!(
                    "Could not cleanly stop isolated backend on port {}",
                    self.port
                );
                if !thread::panicking() {
                    panic!("{message}");
                }
                eprintln!("{message}");
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

pub struct PythonHost {
    pub process: OwnedProcess,
    pub report: Value,
    pub directory: PathBuf,
}
impl PythonHost {
    pub fn start(app: &App, interpreter: &Path) -> Result<Self> {
        app.call("start", &[], 0)?;
        let bundle = app.export()?;
        let config_path = app.directory.join("host-config.json");
        fs::write(
            &config_path,
            serde_json::to_vec(&json!({"bundle":bundle,"port":app.registry_port,
            "directory":app.directory}))?,
        )?;
        let stdout = fs::File::create(app.directory.join("host.stdout"))?;
        let stderr = fs::File::create(app.directory.join("host.stderr"))?;
        let mut command = Command::new(interpreter);
        command
            .args(["-I", "-S", "-X", "utf8"])
            .arg(fixture("python_host.py"))
            .arg(config_path)
            .current_dir(&app.directory)
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        hidden(&mut command);
        let mut process = OwnedProcess(command.spawn()?);
        let ready = app.directory.join("ready.json");
        let mut report = None;
        wait_until(Duration::from_secs(15), || {
            report = fs::read(&ready)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            if report.is_none() {
                anyhow::ensure!(process.0.try_wait()?.is_none(), "Python fixture exited before reporting. Set FLINT_TEST_PYTHON to an interpreter executable. {}\n{}", fs::read_to_string(app.directory.join("host.stderr"))?, fs::read_to_string(app.directory.join("host.stdout"))?);
            }
            Ok(report.is_some())
        })?;
        let report = report.unwrap();
        anyhow::ensure!(report.get("error").is_none(), "Host bootstrap: {report}");
        anyhow::ensure!(
            report["pid"].as_u64().is_some_and(|pid| pid > 0),
            "Missing host PID"
        );
        Ok(Self {
            process,
            report,
            directory: app.directory.clone(),
        })
    }
    pub fn drop_execution_connection(&self) -> Result<()> {
        fs::write(self.directory.join("drop-execution"), b"drop")?;
        Ok(())
    }
}
impl Drop for PythonHost {
    fn drop(&mut self) {
        // Windows virtual-environment launchers can own a distinct interpreter child.
        // Ask the fixture itself to exit before dropping the launcher process handle.
        let _ = fs::write(self.directory.join("stop-host"), b"stop");
        let _ = self.process.0.wait_timeout(Duration::from_secs(3));
    }
}
