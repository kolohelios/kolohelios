use crate::term::{BOLD, RED, RESET, YELLOW};

pub fn run(_bucket: &str) {
    eprintln!("{RED}{BOLD}error:{RESET} {YELLOW}object-store status not yet implemented{RESET}");
    std::process::exit(1);
}
