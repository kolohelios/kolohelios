use crate::term::{BOLD, RED, RESET, YELLOW};

pub fn emit(_module: &str, _force: bool) {
    eprintln!(
        "{RED}{BOLD}error:{RESET} {YELLOW}object-store tfstate emit not yet implemented{RESET}"
    );
    std::process::exit(1);
}

pub fn migrate(_module: &str) {
    eprintln!(
        "{RED}{BOLD}error:{RESET} {YELLOW}object-store tfstate migrate not yet implemented{RESET}"
    );
    std::process::exit(1);
}
