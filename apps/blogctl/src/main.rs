use std::process::ExitCode;

fn main() -> ExitCode {
    match blogctl::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
