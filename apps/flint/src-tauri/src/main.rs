mod cli;
mod desktop;

fn main() {
    let code = cli::run();
    std::process::exit(code);
}
