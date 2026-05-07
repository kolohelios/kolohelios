use std::io::Write;
use std::path::{Path, PathBuf};

use crate::object_store::client::{self, BucketKey};
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(cluster: &str, bucket: &str, output: Option<PathBuf>) {
    let output = output.unwrap_or_else(|| default_output_path(bucket));

    let token = match client::token_from_env() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    println!(
        "{BOLD}object-store init{RESET}  cluster={DIM}{cluster}{RESET} bucket={DIM}{bucket}{RESET}"
    );

    match client::get_bucket(&token, cluster, bucket) {
        Ok(Some(_)) => {
            println!(
                "  {GREEN}{BOLD}ok{RESET}  bucket already exists in {cluster} — nothing to do"
            );
            println!(
                "  {YELLOW}note:{RESET} access keys are only returned at creation; if you've \
                lost them, delete and recreate via the Linode console"
            );
            return;
        }
        Ok(None) => {} // need to create
        Err(e) => {
            eprintln!("  {RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    }

    println!("  {DIM}creating bucket...{RESET}");
    if let Err(e) = client::create_bucket(&token, cluster, bucket) {
        eprintln!("  {RED}{BOLD}error:{RESET} {e}");
        std::process::exit(1);
    }
    println!("  {GREEN}{BOLD}ok{RESET}  bucket created");

    println!("  {DIM}minting bucket-scoped access key...{RESET}");
    let key_label = format!("{bucket}-monorepo");
    let key = match client::create_bucket_key(&token, cluster, bucket, &key_label) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("  {RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = write_credentials_file(&output, &key) {
        eprintln!(
            "{RED}{BOLD}error:{RESET} bucket and key created but writing credentials \
            to {} failed: {e}\n\
            The secret is unrecoverable from the Linode API. Delete the key in the \
            Linode console and re-run init.",
            output.display()
        );
        std::process::exit(1);
    }

    println!(
        "  {GREEN}{BOLD}ok{RESET}  credentials written to {DIM}{}{RESET} (mode 0600)",
        output.display()
    );
    println!();
    println!("{BOLD}Stash the credentials now — the secret is only returned at creation.{RESET}");
    println!("Use the file (do not paste the values into terminals or chat):");
    println!();
    println!(
        "  {DIM}# load into shell:{RESET}             set -a && source {} && set +a",
        output.display()
    );
    println!(
        "  {DIM}# set GitHub Actions secrets:{RESET}  gh secret set TFSTATE_ACCESS_KEY < <(grep AWS_ACCESS_KEY_ID {} | cut -d= -f2-)",
        output.display()
    );
    println!(
        "  {DIM}#                              {RESET}  gh secret set TFSTATE_SECRET_KEY < <(grep AWS_SECRET_ACCESS_KEY {} | cut -d= -f2-)",
        output.display()
    );
    println!();
    println!(
        "After stashing, delete the file: {DIM}shred -u {}{RESET}",
        output.display()
    );
}

fn default_output_path(bucket: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("shaka").join(format!("{bucket}-credentials.env"))
}

fn write_credentials_file(path: &Path, key: &BucketKey) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Atomic create-with-mode-0600 so the secret never lives at a more
    // permissive mode, even briefly.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    writeln!(f, "AWS_ACCESS_KEY_ID={}", key.access_key)?;
    writeln!(f, "AWS_SECRET_ACCESS_KEY={}", key.secret_key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_credentials_file_creates_with_owner_only_perms_on_unix() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("creds.env");
        let key = BucketKey {
            label: "test".into(),
            access_key: "AKIA-test".into(),
            secret_key: "shh".into(),
        };
        write_credentials_file(&path, &key).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("AWS_ACCESS_KEY_ID=AKIA-test"));
        assert!(body.contains("AWS_SECRET_ACCESS_KEY=shh"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "expected 0600 perms, got {:o}", mode);
        }
    }

    #[test]
    fn default_output_path_uses_xdg_config_home_when_set() {
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: tests in the same binary may run in parallel; this test
        // relies on the env var only being read inside default_output_path
        // and being restored on exit. Run with `--test-threads=1` if other
        // tests in this module read XDG_CONFIG_HOME.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        let p = default_output_path("kolohelios");
        assert_eq!(
            p,
            tmp.path().join("shaka").join("kolohelios-credentials.env")
        );
        unsafe {
            match prev {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }
}
