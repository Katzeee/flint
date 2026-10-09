mod attach;
mod cli;
mod control;
mod desktop;
mod failure;
mod hosts;

fn main() {
    let code = cli::run();
    std::process::exit(code);
}
