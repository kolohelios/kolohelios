use std::path::{Path, PathBuf};

use crate::object_store::registry;
use crate::term::{BOLD, DIM, GREEN, RED, RESET};

const BUCKET: &str = "kolohelios";
const CLUSTER: &str = "us-sea-1";

pub fn emit(module: &str, force: bool) {
    let module_path = PathBuf::from(module);
    if !module_path.is_dir() {
        eprintln!(
            "{RED}{BOLD}error:{RESET} module path does not exist or is not a directory: {module}"
        );
        std::process::exit(1);
    }

    let project_dir = match find_enclosing_project(&module_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "{RED}{BOLD}error:{RESET} no project.cue found in {module} or its ancestors — \
                tfstate emit requires the module to live inside a project that declares its \
                namespace"
            );
            std::process::exit(1);
        }
    };

    let entries = match registry::collect(Path::new(".")) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("{RED}{BOLD}error:{RESET} {err}");
            std::process::exit(1);
        }
    };

    let owned: Vec<_> = entries
        .iter()
        .filter(|e| e.project == project_dir && e.namespace.kind == "tfstate")
        .collect();
    let ns = match owned.as_slice() {
        [] => {
            eprintln!(
                "{RED}{BOLD}error:{RESET} {} declares no tfstate namespace in its project.cue. \
                Add an entry to objectStorage.namespaces with kind \"tfstate\" before running emit.",
                project_dir.display()
            );
            std::process::exit(1);
        }
        [e] => e.namespace.clone(),
        _ => {
            eprintln!(
                "{RED}{BOLD}error:{RESET} {} declares multiple tfstate namespaces — emit cannot \
                pick one. Split the module or remove the extras.",
                project_dir.display()
            );
            std::process::exit(1);
        }
    };

    let backend_path = module_path.join("backend.tf");
    if backend_path.exists() && !force {
        eprintln!(
            "{RED}{BOLD}error:{RESET} {} already exists. Re-run with --force to overwrite.",
            backend_path.display()
        );
        std::process::exit(1);
    }

    let key = format!("{}/{}/terraform.tfstate", ns.kind, ns.name);
    let body = render_backend(BUCKET, &key, CLUSTER);

    if let Err(e) = std::fs::write(&backend_path, body) {
        eprintln!(
            "{RED}{BOLD}error:{RESET} could not write {}: {e}",
            backend_path.display()
        );
        std::process::exit(1);
    }
    println!(
        "{GREEN}{BOLD}wrote{RESET} {}  {DIM}(bucket={BUCKET} key={key} cluster={CLUSTER}){RESET}",
        backend_path.display()
    );
    println!();
    println!("Next steps:");
    println!(
        "  {DIM}1. Export AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY (from `shaka object-store init`).{RESET}"
    );
    println!(
        "  {DIM}2. From {}: tofu init -migrate-state (or run `shaka object-store tfstate migrate --module {module}`).{RESET}",
        module_path.display()
    );
}

pub fn migrate(module: &str) {
    let module_path = PathBuf::from(module);
    if !module_path.is_dir() {
        eprintln!(
            "{RED}{BOLD}error:{RESET} module path does not exist or is not a directory: {module}"
        );
        std::process::exit(1);
    }

    let backend = module_path.join("backend.tf");
    if !backend.exists() {
        eprintln!(
            "{RED}{BOLD}error:{RESET} no backend.tf in {module}. Run \
            `shaka object-store tfstate emit --module {module}` first."
        );
        std::process::exit(1);
    }

    if !crate::object_store::s3::creds_present() {
        eprintln!(
            "{RED}{BOLD}error:{RESET} AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY must be set \
            in the environment for tofu to authenticate against the remote backend."
        );
        std::process::exit(1);
    }

    println!(
        "{BOLD}tfstate migrate{RESET}  module={DIM}{module}{RESET}\n\
         {DIM}running: tofu init -migrate-state{RESET}"
    );

    let status = std::process::Command::new("tofu")
        .current_dir(&module_path)
        .args(["init", "-migrate-state"])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("{GREEN}{BOLD}migrated{RESET} state for {module}");
        }
        Ok(s) => {
            eprintln!("{RED}{BOLD}error:{RESET} tofu init -migrate-state exited with status {s}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} could not spawn tofu: {e}");
            std::process::exit(1);
        }
    }
}

/// Walk up from `start` looking for the nearest directory that contains a
/// `project.cue`. Returns `None` if none is found before the filesystem
/// root.
fn find_enclosing_project(start: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        if dir.join("project.cue").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

fn render_backend(bucket: &str, key: &str, cluster: &str) -> String {
    let endpoint = format!("https://{cluster}.linodeobjects.com");
    format!(
        "# Generated by `shaka object-store tfstate emit`. Do not edit by hand.\n\
         # Re-run emit to regenerate. The bucket and key derive from this\n\
         # module's project.cue declaration.\n\
         \n\
         terraform {{\n\
         \x20\x20backend \"s3\" {{\n\
         \x20\x20\x20\x20bucket = \"{bucket}\"\n\
         \x20\x20\x20\x20key    = \"{key}\"\n\
         \x20\x20\x20\x20region = \"{cluster}\"\n\
         \n\
         \x20\x20\x20\x20endpoints = {{\n\
         \x20\x20\x20\x20\x20\x20s3 = \"{endpoint}\"\n\
         \x20\x20\x20\x20}}\n\
         \n\
         \x20\x20\x20\x20skip_credentials_validation = true\n\
         \x20\x20\x20\x20skip_region_validation      = true\n\
         \x20\x20\x20\x20skip_metadata_api_check     = true\n\
         \x20\x20\x20\x20skip_requesting_account_id  = true\n\
         \x20\x20\x20\x20skip_s3_checksum            = true\n\
         \x20\x20\x20\x20use_path_style              = true\n\
         \x20\x20}}\n\
         }}\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn render_backend_includes_bucket_key_endpoint() {
        let body = render_backend("kolohelios", "tfstate/devbox/terraform.tfstate", "us-sea-1");
        assert!(body.contains("bucket = \"kolohelios\""));
        assert!(body.contains("key    = \"tfstate/devbox/terraform.tfstate\""));
        assert!(body.contains("region = \"us-sea-1\""));
        assert!(body.contains("s3 = \"https://us-sea-1.linodeobjects.com\""));
        assert!(body.contains("use_path_style              = true"));
    }

    #[test]
    fn find_enclosing_project_returns_module_dir_when_it_has_project_cue() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("project.cue"), "package project\n").unwrap();
        assert_eq!(find_enclosing_project(tmp.path()), Some(tmp.path().into()));
    }

    #[test]
    fn find_enclosing_project_walks_up() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("project.cue"), "package project\n").unwrap();
        let nested = tmp.path().join("terraform");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_enclosing_project(&nested), Some(tmp.path().into()));
    }

    #[test]
    fn find_enclosing_project_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        // No project.cue anywhere up to the temp root, so this only finds
        // nothing if no project.cue exists between `nested` and `/`. Since
        // `/` typically lacks one, this is fine.
        assert!(find_enclosing_project(&nested).is_none());
    }
}
