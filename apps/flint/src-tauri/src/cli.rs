use crate::application::{
    self, Application, ExecuteRequest, ExecutionStatus, ExecutionView, ExportTarget, Failure,
    FailureCode, HostKind, Result, StartWorkflowRequest,
};
use clap::{builder::TypedValueParser, Args, Parser, Subcommand};
use serde::Serialize;
use std::{io::Read, path::PathBuf};
use strum::IntoEnumIterator;

#[derive(Parser)]
#[command(name = "flint", version, about = "Application execution bridge")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Start,
    Status,
    Stop,
    Restart,
    Serve,
    Gui,
    Instances {
        #[arg(long = "type")]
        instance_type: Option<String>,
    },
    Hosts {
        #[command(subcommand)]
        command: Option<HostCommand>,
    },
    Workflow {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Exec(Execute),
    Execution {
        #[arg(long)]
        workflow_id: String,
        #[arg(long)]
        execution_id: String,
        #[arg(long, default_value="summary", value_parser=["summary", "full"])]
        view: String,
    },
    Bridge {
        #[command(subcommand)]
        command: BridgeCommand,
    },
    /// Inject the Bridge into a running host process and confirm it connects.
    Attach(Attach),
}
#[derive(Args)]
struct Attach {
    #[arg(long)]
    pid: u32,
    #[arg(
        long,
        value_parser = host_kind_parser(),
        help = "Override the detected host kind"
    )]
    host_kind: Option<HostKind>,
    #[arg(
        long,
        help = "Instance name for the injected Bridge; defaults to the host kind"
    )]
    name: Option<String>,
}
fn host_kind_parser() -> impl TypedValueParser<Value = HostKind> {
    clap::builder::PossibleValuesParser::new(HostKind::iter().map(<&'static str>::from))
        .map(|value| value.parse().expect("validated host kind"))
}

#[derive(Subcommand)]
enum HostCommand {
    /// Inspect a local host process and its window without changing window state.
    Info {
        #[arg(long)]
        pid: u32,
        #[arg(
            long,
            help = "Capture and include a PNG data URL; unavailable captures include a reason"
        )]
        preview: bool,
    },
    /// Restore and focus a local application's window.
    Focus {
        #[arg(long)]
        pid: u32,
    },
}

#[derive(Subcommand)]
enum BridgeCommand {
    /// Write a host's install package or a platform library.
    Export {
        #[arg(value_parser = export_target_parser())]
        target: ExportTarget,
        #[arg(
            long,
            help = "Defaults to flint-<target>.<format> in the current directory"
        )]
        output: Option<PathBuf>,
    },
}
fn export_target_parser() -> impl TypedValueParser<Value = ExportTarget> {
    clap::builder::PossibleValuesParser::new(
        Application::export_targets()
            .into_iter()
            .map(ExportTarget::name),
    )
    .map(|value| value.parse().expect("validated export target"))
}

/// A command that needs the backend is an explicit request to use it.
async fn running_application() -> Result<Application> {
    let application = Application::load()?;
    application.start_backend().await?;
    Ok(application)
}
#[derive(Args)]
#[command(group(clap::ArgGroup::new("source").required(true).args(["code", "file", "stdin"])))]
struct Execute {
    #[arg(long)]
    instance_id: String,
    #[arg(long)]
    workflow_id: String,
    #[arg(long, default_value = "")]
    name: String,
    #[arg(long)]
    code: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    stdin: bool,
}
#[derive(Serialize)]
#[serde(untagged)]
enum Output {
    Status(Option<application::PingResponse>),
    Stopped(application::BackendStopped),
    Instances {
        instances: Vec<application::InstanceInfo>,
    },
    Workflow(application::StartWorkflowResponse),
    Executed {
        workflow_id: String,
        #[serde(flatten)]
        result: application::ExecutionResult,
    },
    Execution(application::GetExecutionResponse),
    Attached(application::AttachResult),
    Exported(application::ExportResult),
    Hosts {
        hosts: Vec<application::HostCandidate>,
    },
    Host(application::HostInfo),
    Focused {
        pid: u32,
        focused: bool,
    },
}
impl Output {
    fn print(self, json: bool) -> Result<i32> {
        let failed = match &self {
            Self::Executed { result, .. } => result.status() == ExecutionStatus::Failed,
            Self::Execution(result) => result.status() == ExecutionStatus::Failed,
            _ => false,
        };
        let text = if json {
            serde_json::to_string(&self)
        } else {
            serde_json::to_string_pretty(&self)
        }
        .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, &error))?;
        println!("{text}");
        Ok(i32::from(failed))
    }
}

fn print_failure(failure: &Failure, json: bool) {
    #[derive(Serialize)]
    struct ErrorOutput<'a> {
        error: &'a Failure,
    }
    if json {
        println!(
            "{}",
            serde_json::to_string(&ErrorOutput { error: failure })
                .expect("failure is serializable")
        );
    } else {
        eprintln!("{failure}");
    }
}

pub fn run() -> i32 {
    let json = std::env::args().any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = e.exit_code();
            if code == 0 || !json {
                let _ = e.print();
            } else {
                print_failure(
                    &Failure::with_message(FailureCode::InvalidArguments, e.to_string()),
                    true,
                );
            }
            return code;
        }
    };
    let json = cli.json;
    let result = run_command(cli.command.unwrap_or(Command::Gui))
        .and_then(|output| output.map_or(Ok(0), |output| output.print(json)));
    match result {
        Ok(code) => code,
        Err(failure) => {
            print_failure(&failure, json);
            if failure.is(FailureCode::Interrupted) {
                130
            } else {
                1
            }
        }
    }
}

fn run_command(command: Command) -> Result<Option<Output>> {
    if matches!(&command, Command::Gui) {
        crate::desktop::run(Application::load()?)
            .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, error.as_ref()))?;
        return Ok(None);
    }
    // File and terminal input belong to the CLI, before any product operation.
    let execute = if let Command::Exec(ref e) = command {
        let (code, filename) = if let Some(path) = &e.file {
            let path = path
                .canonicalize()
                .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, &error))?;
            (
                std::fs::read_to_string(&path)
                    .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, &error))?,
                Some(path.display().to_string()),
            )
        } else if e.stdin {
            let mut code = String::new();
            std::io::stdin()
                .read_to_string(&mut code)
                .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, &error))?;
            (code, None)
        } else {
            (e.code.clone().unwrap(), None)
        };
        Some(ExecuteRequest {
            instance_id: e.instance_id.clone(),
            workflow_id: e.workflow_id.clone(),
            name: e.name.clone(),
            code,
            filename,
        })
    } else {
        None
    };
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, &error))?;
    let result = runtime.block_on(async {
        if matches!(&command, Command::Serve) {
            Application::load()?.serve().await?;
            return Ok(None);
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => Err(Failure::new(FailureCode::Interrupted)),
            result = dispatch(command, execute) => result.map(Some),
        }
    });
    // A native operation that times out must not hold the CLI process open.
    runtime.shutdown_background();
    result
}

async fn dispatch(command: Command, execute: Option<ExecuteRequest>) -> Result<Output> {
    Ok(match command {
        Command::Start => Output::Status(Some(Application::load()?.start_backend().await?)),
        Command::Status => Output::Status(Application::load()?.backend_status().await?),
        Command::Stop => Output::Stopped(Application::load()?.stop_backend().await?),
        Command::Restart => Output::Status(Some(Application::load()?.restart_backend().await?)),
        Command::Instances { instance_type } => Output::Instances {
            instances: running_application()
                .await?
                .instances(instance_type)
                .await?,
        },
        Command::Workflow { name, description } => Output::Workflow(
            running_application()
                .await?
                .create_workflow(StartWorkflowRequest { name, description })
                .await?,
        ),
        Command::Exec(_) => {
            let request = execute.expect("exec input is read before dispatch");
            let workflow_id = request.workflow_id.clone();
            Output::Executed {
                workflow_id,
                result: running_application().await?.execute(request).await?,
            }
        }
        Command::Execution {
            workflow_id,
            execution_id,
            view,
        } => {
            let view = if view == "full" {
                ExecutionView::Full
            } else {
                ExecutionView::Summary
            };
            Output::Execution(
                running_application()
                    .await?
                    .execution(workflow_id, execution_id, view)
                    .await?,
            )
        }
        Command::Attach(a) => Output::Attached(
            Application::load()?
                .attach(a.pid, a.host_kind, a.name)
                .await?,
        ),
        Command::Bridge {
            command: BridgeCommand::Export { target, output },
        } => Output::Exported(Application::export_bridge(target, output).await?),
        Command::Hosts { command, .. } => match command {
            None => Output::Hosts {
                hosts: Application::hosts().await?,
            },
            Some(HostCommand::Info { pid, preview }) => {
                Output::Host(Application::host_info(pid, preview).await?)
            }
            Some(HostCommand::Focus { pid }) => {
                Application::focus_application(pid).await?;
                Output::Focused { pid, focused: true }
            }
        },
        Command::Serve | Command::Gui => unreachable!("handled before dispatch"),
    })
}
