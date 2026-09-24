use crate::bridge_export::BridgeExport;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use flint_control_client::{payload_json, request, status_json, Lifecycle, RemoteError};
use flint_core::{Backend, Config};
use flint_protocol::{envelope::Payload, *};
use std::{io::Read, path::PathBuf};

#[derive(Parser)]
#[command(name = "flint", version, about = "Application execution bridge")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Start(Options),
    Status(Options),
    Stop(Options),
    Restart(Options),
    Serve(Options),
    Gui(Options),
    Instances {
        #[command(flatten)]
        options: Options,
        #[arg(long = "type")]
        instance_type: Option<String>,
    },
    Hosts {
        #[arg(long)]
        json: bool,
    },
    Workflow {
        #[command(flatten)]
        options: Options,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Exec(Execute),
    Execution {
        #[command(flatten)]
        options: Options,
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
}
#[derive(Subcommand)]
enum BridgeCommand {
    Export {
        #[command(subcommand)]
        format: BridgeExport,
    },
}

fn seconds(value: &str) -> std::result::Result<f64, String> {
    let number: f64 = value.parse().map_err(|_| "Expected positive seconds")?;
    if number.is_finite() && number > 0.0 {
        Ok(number)
    } else {
        Err("Expected positive finite seconds".into())
    }
}
#[derive(Args, Clone)]
struct Options {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t=6322, value_parser=clap::value_parser!(u16).range(1..))]
    port: u16,
    #[arg(long, default_value = "127.0.0.1")]
    registry_host: String,
    #[arg(long, default_value_t=6321, value_parser=clap::value_parser!(u16).range(1..))]
    registry_port: u16,
    #[arg(long, default_value="30", value_parser=seconds)]
    timeout: f64,
    #[arg(long)]
    json: bool,
    #[arg(long, help = "Run the backend without a desktop event loop")]
    no_tray: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6322,
            registry_host: "127.0.0.1".into(),
            registry_port: 6321,
            timeout: 30.0,
            json: false,
            no_tray: false,
        }
    }
}
impl Options {
    fn config(&self) -> Config {
        Config {
            host: self.host.clone(),
            port: self.port,
            registry_host: self.registry_host.clone(),
            registry_port: self.registry_port,
            timeout: self.timeout,
            ..Default::default()
        }
    }
}
#[derive(Args)]
#[command(group(clap::ArgGroup::new("source").required(true).args(["code", "file", "stdin"])))]
struct Execute {
    #[command(flatten)]
    options: Options,
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
pub fn run() -> i32 {
    let json = std::env::args().any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = e.exit_code();
            if code == 0 || !json {
                let _ = e.print();
            } else {
                println!(
                    "{}",
                    serde_json::json!({"error_code":"invalid_arguments", "message":e.to_string()})
                );
            }
            return code;
        }
    };
    let result = run_command(cli.command.unwrap_or(Command::Gui(Options::default())));
    match result {
        Ok(Some(value)) => {
            let failed = value.get("status").and_then(|v| v.as_str()) == Some("failed");
            if json {
                println!("{value}");
            } else {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
            if failed {
                1
            } else {
                0
            }
        }
        Ok(None) => 0,
        Err(e) => {
            let remote = e.downcast_ref::<RemoteError>();
            let code = remote.map_or(
                if e.to_string().contains("backend_locked") {
                    "backend_locked"
                } else {
                    "command_failed"
                },
                |e| e.code.as_str(),
            );
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error_code":code,"message":e.to_string()})
                );
            } else {
                eprintln!("{e:#}");
            }
            if code == "interrupted" {
                130
            } else {
                1
            }
        }
    }
}
fn run_command(command: Command) -> Result<Option<serde_json::Value>> {
    if let Command::Bridge {
        command: BridgeCommand::Export { format },
    } = &command
    {
        return Ok(Some(format.write()?));
    }
    if let Command::Hosts { .. } = &command {
        let hosts = flint_connect::discover();
        return Ok(Some(serde_json::json!({"hosts": hosts})));
    }
    let options = match &command {
        Command::Start(o)
        | Command::Status(o)
        | Command::Stop(o)
        | Command::Restart(o)
        | Command::Serve(o)
        | Command::Gui(o) => o,
        Command::Instances { options, .. }
        | Command::Workflow { options, .. }
        | Command::Execution { options, .. } => options,
        Command::Exec(e) => &e.options,
        _ => unreachable!(),
    }
    .clone();
    let config = options.config();
    // Read input before starting any background process.
    let execute = if let Command::Exec(ref e) = command {
        let (code, filename) = if let Some(path) = &e.file {
            let path = path.canonicalize()?;
            (
                std::fs::read_to_string(&path)?,
                Some(path.display().to_string()),
            )
        } else if e.stdin {
            let mut code = String::new();
            std::io::stdin().read_to_string(&mut code)?;
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
    let runtime = tokio::runtime::Runtime::new()?;
    if matches!(command, Command::Serve(_)) {
        let backend = runtime.block_on(Backend::bind(config))?;
        if options.no_tray {
            let stop = backend.handle();
            runtime.spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                let _ = stop.request_stop();
            });
            runtime.block_on(backend.run())?;
        } else {
            crate::desktop::run(backend, runtime)?;
        }
        return Ok(None);
    }
    runtime.block_on(async {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => Err(RemoteError { code:"interrupted".into(), message:"Stopped waiting; submitted host code may still be running".into() }.into()),
            result = async {
                let lifecycle = Lifecycle { config:config.clone(), no_tray:options.no_tray };
                let value = match command {
                    Command::Start(_)|Command::Status(_) => status_json(lifecycle.ensure().await?),
                    Command::Stop(_) => lifecycle.stop().await?,
                    Command::Restart(_) => status_json(lifecycle.restart().await?),
                    other => {
                        lifecycle.ensure().await?;
                        let payload = match other {
                            Command::Instances{instance_type,..} => Payload::ListInstancesRequest(ListInstancesRequest{instance_type}),
                            Command::Workflow{name,description,..} => Payload::StartWorkflowRequest(StartWorkflowRequest{name,description}),
                            Command::Exec(_) => Payload::ExecuteRequest(execute.clone().unwrap()),
                            Command::Execution{workflow_id,execution_id,view,..} => Payload::GetExecutionRequest(GetExecutionRequest{workflow_id,execution_id,view:(if view=="full" {ExecutionView::Full} else {ExecutionView::Summary}) as i32}),
                            Command::Gui(_) => Payload::ShowWindowRequest(ShowWindowRequest{}),
                            _ => unreachable!(),
                        };
                        let mut value = payload_json(request(&config, payload).await?)?;
                        if let Some(e) = execute { value["workflow_id"] = e.workflow_id.into(); }
                        value
                    }
                };
                Ok(Some(value))
            } => result,
        }
    })
}
