use std::env;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    ExitCode::from(verisilicon_sr::cli::run(
        env::args_os().skip(1),
        &mut stdout,
        &mut stderr,
    ))
}
