use super::*;
use flint_contracts::host_settings::HostSettings;
use serde_json::json;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    process::{Child, Command, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

struct PythonProcess(Child);

impl Drop for PythonProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            if !matches!(self.0.wait_timeout(Duration::from_secs(3)), Ok(Some(_))) {
                if std::thread::panicking() {
                    eprintln!("Python bootstrap process did not terminate during cleanup");
                } else {
                    panic!("Python bootstrap process did not terminate during cleanup");
                }
            }
        }
    }
}

fn captured(mut file: File) -> Vec<u8> {
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut output = Vec::new();
    file.take(64 * 1024).read_to_end(&mut output).unwrap();
    output
}

#[test]
#[ignore = "requires Python; run `cargo xtask test python`"]
fn loader_hands_the_request_to_the_platform_entry() {
    let settings = HostSettings {
        address: "127.0.0.1".into(),
        port: 6321,

        name: "a\"b\\c\n场景".into(),
    };
    let root = r"C:\tools\flint-bridge";
    let error_path = r"C:\temp\42.error";
    let plan = CpythonPlan {
        import_root: root.into(),
        module: "flint_bridge.maya".into(),
        settings,
    };
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut process = PythonProcess(
        Command::new("python")
            .args(["-I", "-X", "utf8", "-c", include_str!("tests/python_bootstrap.py")])
            .arg(source(&plan, Path::new(error_path)).unwrap())
            .stdin(Stdio::null())
            .stdout(stdout.try_clone().unwrap())
            .stderr(stderr.try_clone().unwrap())
            .spawn()
            .expect("The Python test suite supplies its interpreter on PATH"),
    );
    let completed = process.0.wait_timeout(Duration::from_secs(10)).unwrap();
    let timed_out = completed.is_none();
    let status = completed.unwrap_or_else(|| {
        process.0.kill().unwrap();
        process
            .0
            .wait_timeout(Duration::from_secs(3))
            .unwrap()
            .expect("Python bootstrap did not terminate after its deadline")
    });
    let stdout = captured(stdout);
    let stderr = captured(stderr);
    let diagnostics = format!(
        "program=python, status={status}, timed_out={timed_out}\nPATH={}\nstdout:\n{}\nstderr:\n{}",
        std::env::var_os("PATH").unwrap_or_default().to_string_lossy(),
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr),
    );
    assert!(!timed_out && status.success(), "{diagnostics}");
    let observed: serde_json::Value = serde_json::from_slice(&stdout)
        .unwrap_or_else(|error| panic!("Invalid bootstrap response: {error}\n{diagnostics}"));
    assert_eq!(
        observed,
        json!({
            "import_root": root,
            "error_path": error_path,
            "request": {
                "module": "flint_bridge.maya",
                "import_root": root,
                "settings": {
                    "address": "127.0.0.1",
                    "port": 6321,
                    "name": "a\"b\\c\n场景",
                },
            },
        })
    );
}
