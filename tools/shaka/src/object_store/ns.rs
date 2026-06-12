use std::collections::BTreeSet;
use std::path::Path;

use crate::object_store::{registry, s3};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn list() {
    let entries = match registry::collect(Path::new(".")) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("{RED}{BOLD}error:{RESET} {}", err);
            std::process::exit(1);
        }
    };

    if entries.is_empty() {
        println!("{YELLOW}no namespaces declared in any project.cue{RESET}");
        return;
    }

    println!("{BOLD}namespaces{RESET} ({} total)", entries.len());
    for e in &entries {
        println!(
            "  {BOLD}{}{RESET}  {DIM}{}{RESET}",
            e.namespace.prefix(),
            e.project.display()
        );
        println!("    {DIM}{}{RESET}", e.namespace.purpose);
    }
}

pub fn audit(bucket: &str, cluster: &str) {
    let entries = match registry::collect(Path::new(".")) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("{RED}{BOLD}error:{RESET} {}", err);
            std::process::exit(1);
        }
    };

    let mut errors = registry::validate_uniqueness(&entries);

    let bucket_check = audit_bucket(&entries, bucket, cluster);
    let mut bucket_errors = match bucket_check {
        AuditBucketResult::Skipped(reason) => {
            println!("{YELLOW}bucket drift check skipped:{RESET} {DIM}{reason}{RESET}");
            Vec::new()
        }
        AuditBucketResult::Ok { orphans, empty } => {
            let mut errs = Vec::new();
            for orphan in &orphans {
                errs.push(format!(
                    "bucket prefix '{orphan}' has no matching namespace declaration"
                ));
            }
            for empty_prefix in &empty {
                errs.push(format!(
                    "namespace '{empty_prefix}' is declared but has no objects in the bucket \
                    (informational — emit/migrate to populate it)"
                ));
            }
            errs
        }
        AuditBucketResult::Error(msg) => {
            eprintln!("{RED}{BOLD}error:{RESET} bucket query failed: {msg}");
            std::process::exit(1);
        }
    };

    errors.append(&mut bucket_errors);

    if errors.is_empty() {
        println!(
            "{GREEN}{BOLD}registry ok{RESET} ({} namespace{} declared)",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        );
        return;
    }

    eprintln!("{RED}{BOLD}registry errors:{RESET}");
    for e in &errors {
        eprintln!("  {RED}-{RESET} {e}");
    }
    std::process::exit(1);
}

enum AuditBucketResult {
    Skipped(String),
    Ok {
        orphans: BTreeSet<String>,
        empty: BTreeSet<String>,
    },
    Error(String),
}

fn audit_bucket(entries: &[registry::Entry], bucket: &str, cluster: &str) -> AuditBucketResult {
    if !s3::aws_available() {
        return AuditBucketResult::Skipped(
            "aws CLI not on PATH (run `nix shell nixpkgs#awscli2` for the bucket-side audit)"
                .into(),
        );
    }
    if !s3::creds_present() {
        return AuditBucketResult::Skipped(
            "AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set".into(),
        );
    }
    let actual = match s3::list_namespace_prefixes(bucket, cluster) {
        Ok(p) => p,
        Err(e) => return AuditBucketResult::Error(e.to_string()),
    };
    let declared: BTreeSet<String> = entries.iter().map(|e| e.namespace.prefix()).collect();
    let orphans = actual.difference(&declared).cloned().collect();
    let empty = declared.difference(&actual).cloned().collect();
    AuditBucketResult::Ok { orphans, empty }
}
