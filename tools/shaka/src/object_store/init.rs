use crate::object_store::client;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub fn run(cluster: &str, bucket: &str) {
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

    println!(
        "  {GREEN}{BOLD}ok{RESET}  access key '{}' minted",
        key.label
    );
    println!();
    println!("{BOLD}Stash these credentials NOW — Linode does not return the secret again.{RESET}");
    println!();
    println!("  {BOLD}AWS_ACCESS_KEY_ID{RESET}={}", key.access_key);
    println!("  {BOLD}AWS_SECRET_ACCESS_KEY{RESET}={}", key.secret_key);
    println!();
    println!("Set them as repository secrets in GitHub Actions:");
    println!(
        "  {DIM}gh secret set TFSTATE_ACCESS_KEY --body '{}'{RESET}",
        key.access_key
    );
    println!("  {DIM}gh secret set TFSTATE_SECRET_KEY --body '<secret>'{RESET}");
    println!();
    println!("And export them in your local shell (or stash in a secret manager) when running tofu against the remote backend.");
}
