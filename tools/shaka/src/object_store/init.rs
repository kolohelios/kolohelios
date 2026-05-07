use crate::term::{BOLD, RED, RESET, YELLOW};

pub fn run(_cluster: &str, _bucket: &str) {
    eprintln!("{RED}{BOLD}error:{RESET} {YELLOW}object-store init not yet implemented{RESET}");
    std::process::exit(1);
}
