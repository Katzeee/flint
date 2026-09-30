mod bridge_export;
mod cli;
mod desktop;
mod hosts;

fn main() {
    let code = cli::run();
    std::process::exit(code);
}
