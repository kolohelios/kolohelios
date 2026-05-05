use std::fs;
use std::path::{Path, PathBuf};

use crate::project::{audit, generate_justfiles, schema_check};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

const SLOTS: &[&str] = &["apps", "packages", "projects", "tools"];

pub fn run(name: String, slot: String) {
    if let Err(e) = validate_name(&name) {
        die(&e);
    }
    if let Err(e) = validate_slot(&slot) {
        die(&e);
    }

    let project_dir = PathBuf::from(&slot).join(&name);
    if project_dir.exists() {
        die(&format!("{} already exists", project_dir.display()));
    }

    if let Err(e) = scaffold_rust(&project_dir, &name) {
        die(&format!(
            "could not scaffold {}: {e}",
            project_dir.display()
        ));
    }

    println!(
        "{GREEN}{BOLD}scaffolded{RESET} {name} at {}",
        project_dir.display()
    );

    schema_check::run();
    generate_justfiles::run(false);
    audit::run();
}

fn validate_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("name must not be empty".into());
    };
    if !first.is_ascii_lowercase() {
        return Err(format!(
            "name '{name}' must start with a lowercase letter (a-z)"
        ));
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!(
                "name '{name}' contains invalid character '{c}' (allowed: a-z, 0-9, -)"
            ));
        }
    }
    Ok(())
}

fn validate_slot(slot: &str) -> Result<(), String> {
    if SLOTS.contains(&slot) {
        Ok(())
    } else {
        Err(format!(
            "invalid slot '{slot}' (allowed: {})",
            SLOTS.join(", ")
        ))
    }
}

fn scaffold_rust(dir: &Path, name: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir.join("src"))?;
    fs::write(dir.join("project.cue"), project_cue(name))?;
    fs::write(dir.join("Cargo.toml"), cargo_toml(name))?;
    fs::write(dir.join("flake.nix"), flake_nix(name))?;
    fs::write(dir.join(".envrc"), ENVRC)?;
    fs::write(dir.join("README.md"), readme(name))?;
    fs::write(dir.join(".gitignore"), GITIGNORE)?;
    fs::write(dir.join("src/main.rs"), main_rs(name))?;
    Ok(())
}

fn project_cue(name: &str) -> String {
    format!(
        "package project\n\
         \n\
         #Project & {{\n\
         \tname: \"{name}\"\n\
         \tkind: \"rust\"\n\
         \tcoverage: {{\n\
         \t\tline: {{\n\
         \t\t\tfail: 1\n\
         \t\t}}\n\
         \t\tbranch: {{\n\
         \t\t\tfail: 1\n\
         \t\t}}\n\
         \t}}\n\
         }}\n"
    )
}

fn cargo_toml(name: &str) -> String {
    format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         license = \"MIT OR Apache-2.0\"\n\
         publish = false\n\
         \n\
         [[bin]]\n\
         name = \"{name}\"\n\
         path = \"src/main.rs\"\n"
    )
}

fn flake_nix(name: &str) -> String {
    format!(
        r#"{{
  description = "{name}";

  inputs = {{
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
    rust-overlay = {{
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    }};
  }};

  outputs =
    {{
      self,
      kolohelios-nix,
      nixpkgs,
      rust-overlay,
      ...
    }}:
    let
      inherit (kolohelios-nix.lib) supportedSystems workflowPackages;

      forEachSupportedSystem =
        f:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {{
            inherit system;
            pkgs = import nixpkgs {{
              inherit system;
              config.allowUnfree = true;
              overlays = [ rust-overlay.overlays.default ];
            }};
          }}
        );

      rustToolchain =
        pkgs:
        pkgs.rust-bin.nightly.latest.default.override {{
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools-preview"
          ];
        }};
    in
    {{
      devShells = forEachSupportedSystem (
        {{ pkgs, ... }}:
        {{
          default = pkgs.mkShell {{
            packages = [
              (rustToolchain pkgs)
            ]
            ++ (workflowPackages pkgs)
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;
          }};
        }}
      );

      formatter = kolohelios-nix.formatter;
    }};
}}
"#
    )
}

const ENVRC: &str = "use flake\n";

const GITIGNORE: &str = "target/\nresult\n.direnv/\n";

fn readme(name: &str) -> String {
    format!("# {name}\n")
}

fn main_rs(name: &str) -> String {
    format!(
        "fn main() {{\n\
         \x20   println!(\"hello from {name}\");\n\
         }}\n\
         \n\
         #[cfg(test)]\n\
         mod tests {{\n\
         \x20   #[test]\n\
         \x20   fn placeholder() {{\n\
         \x20       assert_eq!(2 + 2, 4);\n\
         \x20   }}\n\
         }}\n"
    )
}

fn die(msg: &str) -> ! {
    eprintln!("{RED}{BOLD}error:{RESET} {msg}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_simple_lowercase() {
        assert!(validate_name("foo").is_ok());
    }

    #[test]
    fn validate_name_accepts_hyphenated_and_digits() {
        assert!(validate_name("my-project-2").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn validate_name_rejects_uppercase() {
        assert!(validate_name("Foo").is_err());
    }

    #[test]
    fn validate_name_rejects_leading_digit() {
        assert!(validate_name("1foo").is_err());
    }

    #[test]
    fn validate_name_rejects_leading_hyphen() {
        assert!(validate_name("-foo").is_err());
    }

    #[test]
    fn validate_name_rejects_underscore() {
        assert!(validate_name("foo_bar").is_err());
    }

    #[test]
    fn validate_slot_accepts_each_documented_slot() {
        for s in SLOTS {
            assert!(validate_slot(s).is_ok(), "slot {s} should be accepted");
        }
    }

    #[test]
    fn validate_slot_rejects_infra() {
        // infra is a real slot in the repo but out of scope for `new`
        // until we ship infra scaffolding (#23 follow-up).
        assert!(validate_slot("infra").is_err());
    }

    #[test]
    fn validate_slot_rejects_unknown() {
        assert!(validate_slot("services").is_err());
        assert!(validate_slot("").is_err());
    }

    #[test]
    fn project_cue_includes_name_and_kind() {
        let out = project_cue("demo");
        assert!(out.contains("name: \"demo\""));
        assert!(out.contains("kind: \"rust\""));
        assert!(out.contains("coverage:"));
    }

    #[test]
    fn cargo_toml_uses_dual_license() {
        let out = cargo_toml("demo");
        assert!(out.contains(r#"license = "MIT OR Apache-2.0""#));
        assert!(out.contains(r#"name = "demo""#));
    }

    #[test]
    fn main_rs_includes_cfg_test_module() {
        // rust-has-tests audit rule looks for #[cfg(test)] in any .rs file.
        let out = main_rs("demo");
        assert!(out.contains("#[cfg(test)]"));
    }
}
