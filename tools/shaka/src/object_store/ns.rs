use crate::term::{BOLD, RED, RESET, YELLOW};

pub fn list() {
    eprintln!("{RED}{BOLD}error:{RESET} {YELLOW}object-store ns list not yet implemented{RESET}");
    std::process::exit(1);
}

pub fn audit(_bucket: &str) {
    eprintln!("{RED}{BOLD}error:{RESET} {YELLOW}object-store ns audit not yet implemented{RESET}");
    std::process::exit(1);
}
