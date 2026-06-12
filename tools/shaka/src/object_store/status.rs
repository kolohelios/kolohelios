use std::collections::BTreeSet;
use std::path::Path;

use crate::object_store::{client, registry, s3};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(bucket: &str, cluster: &str) {
    println!(
        "{BOLD}object-store status{RESET}  bucket={DIM}{bucket}{RESET} cluster={DIM}{cluster}{RESET}"
    );
    println!();

    bucket_section(bucket, cluster);
    println!();
    registry_section(bucket, cluster);
}

fn bucket_section(bucket: &str, cluster: &str) {
    println!("{BOLD}bucket{RESET}");
    let token = match client::token_from_env() {
        Ok(t) => t,
        Err(_) => {
            println!(
                "  {YELLOW}skipped{RESET}  {DIM}LINODE_TOKEN not set; cannot query Linode API{RESET}"
            );
            return;
        }
    };
    match client::get_bucket(&token, cluster, bucket) {
        Ok(Some(body)) => {
            let hostname = body
                .get("hostname")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            let size = body
                .get("size")
                .and_then(|v| v.as_u64())
                .map(|n| format!("{n} bytes"))
                .unwrap_or_else(|| "(unknown size)".into());
            println!("  {GREEN}{BOLD}exists{RESET}  hostname={DIM}{hostname}{RESET} size={DIM}{size}{RESET}");
        }
        Ok(None) => {
            println!(
                "  {RED}{BOLD}missing{RESET}  bucket not found in {cluster} — \
                run `shaka object-store init` to create it"
            );
        }
        Err(e) => {
            println!("  {RED}{BOLD}error{RESET}  {e}");
        }
    }
}

fn registry_section(bucket: &str, cluster: &str) {
    println!("{BOLD}registry{RESET}");
    let entries = match registry::collect(Path::new(".")) {
        Ok(e) => e,
        Err(err) => {
            println!("  {RED}{BOLD}error{RESET}  {err}");
            return;
        }
    };
    if entries.is_empty() {
        println!("  {DIM}no namespaces declared{RESET}");
    } else {
        for e in &entries {
            println!(
                "  {BOLD}{}{RESET}  {DIM}{}{RESET}",
                e.namespace.prefix(),
                e.project.display()
            );
        }
    }

    println!();
    println!("{BOLD}drift{RESET}");
    if !s3::aws_available() {
        println!(
            "  {YELLOW}skipped{RESET}  {DIM}aws CLI not on PATH (run `nix shell nixpkgs#awscli2`){RESET}"
        );
        return;
    }
    if !s3::creds_present() {
        println!(
            "  {YELLOW}skipped{RESET}  {DIM}AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set{RESET}"
        );
        return;
    }
    let actual = match s3::list_namespace_prefixes(bucket, cluster) {
        Ok(p) => p,
        Err(e) => {
            println!("  {RED}{BOLD}error{RESET}  {e}");
            return;
        }
    };
    let declared: BTreeSet<String> = entries.iter().map(|e| e.namespace.prefix()).collect();
    let orphans: Vec<_> = actual.difference(&declared).collect();
    let empty: Vec<_> = declared.difference(&actual).collect();

    if orphans.is_empty() && empty.is_empty() {
        println!(
            "  {GREEN}{BOLD}clean{RESET}  every prefix has a declaration and every declaration is populated"
        );
        return;
    }
    for o in &orphans {
        println!("  {RED}orphan{RESET}     {o} (in bucket, no declaration)");
    }
    for e in &empty {
        println!("  {YELLOW}unpopulated{RESET} {e} (declared, no objects)");
    }
}
