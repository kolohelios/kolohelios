//! Notes operations use the existing owner-session wire contract. Secrets
//! stay in memory and child stdin, never argv, environment, files, or logs.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use clap::{Args, Subcommand};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use snafu::{ResultExt, Snafu};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Args)]
pub struct ConnectionOptions {
    /// Project holding wrangler.toml vars and SESSION_SECRET's op:// ref
    #[arg(long, default_value = "services/notes-sync", global = true)]
    project: PathBuf,
    /// Override the backend origin (HTTPS only)
    #[arg(long, global = true)]
    url: Option<String>,
}
#[derive(Subcommand)]
pub enum NotesCommand {
    /// Read-only drift scan; exits nonzero on stale/unavailable artifacts
    Verify {
        #[command(flatten)]
        connection: ConnectionOptions,
        /// Queue stale rows for repair, preserving hot-editor conflicts
        #[arg(long)]
        repair: bool,
        /// Wait for queued work, then scan again; requires --repair
        #[arg(long, requires = "repair")]
        wait: bool,
        /// Maximum queue-wait time, not a background polling schedule
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Create/update the notes-store push hook with a derived signing key
    WebhookInstall {
        #[command(flatten)]
        connection: ConnectionOptions,
    },
    /// Search the shared vault index
    Search {
        #[command(flatten)]
        connection: ConnectionOptions,
        query: String,
        #[arg(long)]
        semantic: bool,
    },
}
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("notes operation I/O failed: {source}"))]
    Io { source: std::io::Error },
    #[snafu(display("notes configuration invalid: {message}"))]
    Config { message: String },
    #[snafu(display("{command} failed (output withheld to protect credentials)"))]
    Command { command: String },
    #[snafu(display("notes response could not be decoded: {source}"))]
    Json { source: serde_json::Error },
    #[snafu(display("vault still has drift, source conflicts, or unavailable artifacts"))]
    Drift,
    #[snafu(display("timed out waiting for notes derivation jobs"))]
    Timeout,
}
fn invalid(message: &str) -> Error {
    Error::Config {
        message: message.into(),
    }
}

fn child(command: &mut Command, input: Option<&[u8]>, label: &str) -> Result<Vec<u8>, Error> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context(IoSnafu)?;
    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| invalid("child stdin unavailable"))?;
        if let Err(source) = stdin.write_all(input) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Io { source });
        }
    }
    let output = child.wait_with_output().context(IoSnafu)?;
    if !output.status.success() {
        return Err(Error::Command {
            command: label.into(),
        });
    }
    Ok(output.stdout)
}

fn cookie(did: &str, expiry: i64, secret: &[u8]) -> Result<String, Error> {
    let payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&json!({"did":did, "exp":expiry})).context(JsonSnafu)?);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).map_err(|_| invalid("invalid signing key"))?;
    mac.update(payload.as_bytes());
    Ok(format!(
        "{payload}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    ))
}
fn webhook_key(secret: &[u8]) -> Result<String, Error> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).map_err(|_| invalid("invalid signing key"))?;
    mac.update(b"notes-store-webhook-v1");
    Ok(format!("{:x}", mac.finalize().into_bytes()))
}
struct Connection {
    url: String,
    owner: String,
    repository: String,
    secret: Vec<u8>,
}
impl Connection {
    fn load(options: ConnectionOptions) -> Result<Self, Error> {
        let project = if options.project.is_absolute() {
            options.project
        } else {
            crate::jj::repo_root()
                .map_err(|_| invalid("run inside the repository or pass an absolute --project"))?
                .join(options.project)
        };
        let config = std::fs::read_to_string(project.join("wrangler.toml")).context(IoSnafu)?;
        let config: toml::Value =
            toml::from_str(&config).map_err(|_| invalid("invalid wrangler.toml"))?;
        let var = |key: &str| {
            config
                .get("vars")
                .and_then(|vars| vars.get(key))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| invalid("required Worker var missing"))
        };
        let url = options.url.unwrap_or(var("OAUTH_BASE_URL")?);
        if !url.starts_with("https://")
            || url.len() <= 8
            || url.contains(['\r', '\n', '?', '#', '@', '"', ' '])
        {
            return Err(invalid(
                "backend URL must be an HTTPS origin without credentials/query/fragment",
            ));
        }
        let refs = std::fs::read_to_string(project.join(".env.example")).context(IoSnafu)?;
        let reference = refs
            .lines()
            .filter_map(|line| line.trim().split_once('='))
            .find(|(key, _)| key.trim() == "SESSION_SECRET")
            .map(|(_, value)| value.trim().trim_matches(['\'', '"']))
            .filter(|reference| reference.starts_with("op://") && !reference.contains(['\r', '\n']))
            .ok_or_else(|| invalid("SESSION_SECRET must be an op:// reference"))?;
        let secret = child(
            Command::new("op").args(["read", "--no-newline", reference]),
            None,
            "op read",
        )?;
        if secret.is_empty() {
            return Err(invalid("SESSION_SECRET is empty"));
        }
        Ok(Self {
            url: url.trim_end_matches('/').into(),
            owner: var("OWNER_DID")?,
            repository: format!("{}/{}", var("GITHUB_OWNER")?, var("GITHUB_REPO")?),
            secret,
        })
    }
    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Error> {
        let cookie = cookie(
            &self.owner,
            chrono::Utc::now().timestamp() + 300,
            &self.secret,
        )?;
        // curl config consumes quoted values on stdin. JSON quoting is a
        // compatible subset for these fixed headers and serialized bodies.
        let quote = |value: &str| serde_json::to_string(value).context(JsonSnafu);
        let mut config = format!(
            "silent\nshow-error\nfail\nmax-time = 120\nurl = {}\nrequest = {}\nheader = {}\n",
            quote(&format!("{}{path}", self.url))?,
            quote(method)?,
            quote(&format!("Cookie: session={cookie}"))?
        );
        if let Some(body) = body {
            config.push_str(&format!(
                "header = \"Content-Type: application/json\"\ndata = {}\n",
                quote(&body.to_string())?
            ));
        }
        let response = child(
            Command::new("curl")
                .args(["--disable", "--config", "-"])
                .env_remove("SSLKEYLOGFILE"),
            Some(config.as_bytes()),
            "notes API request",
        )?;
        serde_json::from_slice(&response).context(JsonSnafu)
    }
    fn scan(&self, repair: bool) -> Result<Value, Error> {
        let mut rows = Vec::new();
        let mut after: Option<String> = None;
        let mut all_clean = true;
        loop {
            let response = self.request(
                "POST",
                "/vault/verify",
                Some(&json!({"repair":repair, "after":after})),
            )?;
            all_clean &= response
                .get("clean")
                .and_then(Value::as_bool)
                .ok_or_else(|| invalid("verification status missing"))?;
            let page = response
                .get("rows")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("verification rows missing"))?;
            if !page
                .iter()
                .all(|row| row.get("reasons").and_then(Value::as_array).is_some())
            {
                return Err(invalid("verification reasons missing"));
            }
            rows.extend(page.iter().cloned());
            let next = response.get("next_after").filter(|next| !next.is_null());
            match next {
                None => break,
                Some(Value::String(next)) if after.as_ref().is_none_or(|after| next > after) => {
                    after = Some(next.clone())
                }
                _ => return Err(invalid("invalid verification cursor")),
            }
        }
        let clean = all_clean
            && rows
                .iter()
                .all(|row| row["reasons"].as_array().is_some_and(Vec::is_empty));
        Ok(json!({"clean":clean, "checked":rows.len(), "rows":rows}))
    }
    fn verify(&self, repair: bool, wait: bool, timeout: u64) -> Result<(), Error> {
        let initial = self.scan(repair)?;
        let report = if wait {
            let start = Instant::now();
            loop {
                let status = self.request("GET", "/vault/status", None)?;
                let pending = status
                    .get("pending")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("queue status missing"))?;
                if pending == 0 {
                    break;
                }
                if start.elapsed() >= Duration::from_secs(timeout) {
                    return Err(Error::Timeout);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            self.scan(false)?
        } else {
            initial
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).context(JsonSnafu)?
        );
        if report["clean"] != true {
            return Err(Error::Drift);
        }
        Ok(())
    }
    fn install(&self) -> Result<(), Error> {
        if self
            .request("GET", "/vault/status", None)?
            .get("pending")
            .and_then(Value::as_u64)
            .is_none()
        {
            return Err(invalid("deploy the notes API before installing its hook"));
        }
        let endpoint = format!("{}/vault/events/github", self.url);
        let resource = format!("repos/{}/hooks", self.repository);
        let response = child(
            Command::new("gh").args(["api", &resource, "--paginate", "--slurp"]),
            None,
            "list repository hooks",
        )?;
        let pages: Vec<Vec<Value>> = serde_json::from_slice(&response).context(JsonSnafu)?;
        let existing: Vec<_> = pages
            .iter()
            .flatten()
            .filter(|hook| hook["config"]["url"] == endpoint)
            .collect();
        if existing.len() > 1 {
            return Err(invalid(
                "multiple matching hooks; remove duplicates before installing",
            ));
        }
        let (method, resource) = match existing.first() {
            Some(hook) => (
                "PATCH",
                format!(
                    "{resource}/{}",
                    hook["id"]
                        .as_u64()
                        .ok_or_else(|| invalid("hook id missing"))?
                ),
            ),
            None => ("POST", resource),
        };
        let payload = json!({"name":"web", "active":true, "events":["push"], "config":{
            "url":endpoint, "content_type":"json", "insecure_ssl":"0", "secret":webhook_key(&self.secret)?
        }});
        child(
            Command::new("gh").args(["api", "--method", method, &resource, "--input", "-"]),
            Some(payload.to_string().as_bytes()),
            "configure repository hook",
        )?;
        println!("notes-store push hook configured");
        Ok(())
    }
}
fn execute(command: NotesCommand) -> Result<(), Error> {
    match command {
        NotesCommand::Verify {
            connection,
            repair,
            wait,
            timeout,
        } => Connection::load(connection)?.verify(repair, wait, timeout),
        NotesCommand::WebhookInstall { connection } => Connection::load(connection)?.install(),
        NotesCommand::Search {
            connection,
            query,
            semantic,
        } => {
            let response = Connection::load(connection)?.request(
                "POST",
                "/vault/search",
                Some(&json!({"query":query,"mode":if semantic {"semantic"} else {"text"}})),
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&response).context(JsonSnafu)?
            );
            Ok(())
        }
    }
}
pub fn run(command: NotesCommand) {
    if let Err(error) = execute(command) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cookie_and_hook_key_match_the_backend_contract() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes/auth.json");
        let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let secret = fixture["secret"].as_str().unwrap().as_bytes();
        assert_eq!(
            cookie(
                fixture["did"].as_str().unwrap(),
                fixture["exp"].as_i64().unwrap(),
                secret
            )
            .unwrap(),
            fixture["cookie"]
        );
        assert_eq!(webhook_key(secret).unwrap(), fixture["webhook_key"]);
    }
}
