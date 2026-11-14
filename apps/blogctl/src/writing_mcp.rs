//! Closed, read-only stdio MCP surface. No generation, review, migration, or
//! subprocess arguments can be requested through tools.
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::writing::{self, Store};

const LIMIT: u64 = 1024 * 1024;

#[derive(Debug, clap::Args)]
pub struct McpArgs {
    #[command(subcommand)]
    pub action: McpAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum McpAction {
    /// Start the read-only stdio adapter in an explicit private working directory.
    Serve {
        #[arg(long)]
        workdir: std::path::PathBuf,
        /// Trusted executable returning a JSON model array (no arguments).
        #[arg(long)]
        models_command: Option<std::path::PathBuf>,
    },
    /// Merge the owned writing server into Pi/Codex, preserving unrelated settings.
    Install {
        #[arg(long)]
        home: std::path::PathBuf,
        #[arg(long)]
        command: std::path::PathBuf,
        #[arg(long)]
        workdir: std::path::PathBuf,
        #[arg(long)]
        models_command: Option<std::path::PathBuf>,
    },
}

pub fn command(args: McpArgs) -> crate::error::Result<()> {
    use snafu::ResultExt;
    match args.action {
        McpAction::Serve {
            workdir,
            models_command,
        } => serve(&workdir, models_command.as_deref()),
        McpAction::Install {
            home,
            command,
            workdir,
            models_command,
        } => crate::agent_config::install(&home, &command, &workdir, models_command.as_deref()),
    }
    .context(crate::error::WritingSnafu)
}

pub fn serve(workdir: &Path, models_command: Option<&Path>) -> writing::Result<()> {
    let store = Store::open(workdir)?;
    run(&store, io::stdin().lock(), io::stdout().lock(), || {
        if let Some(path) = models_command {
            // The configured executable is a trusted invocation boundary, not
            // a tool argument. Its only output contract is a JSON model array.
            let output = Command::new(path)
                .stdin(Stdio::null())
                .output()
                .map_err(|_| ())?;
            if !output.status.success() || output.stdout.len() > LIMIT as usize {
                return Err(());
            }
            let ids: Vec<String> = serde_json::from_slice(&output.stdout).map_err(|_| ())?;
            if ids
                .iter()
                .any(|id| id.trim().is_empty() || id.chars().any(char::is_control))
            {
                return Err(());
            }
            Ok(ids)
        } else {
            crate::novelai::Client::from_env()
                .and_then(|client| client.models())
                .map_err(|_| ())
        }
    })
    .map_err(|source| writing::Error::Io {
        path: "<MCP stdio>".into(),
        source,
    })
}

fn tools() -> Value {
    let definitions = [
        ("writing_list_experiments", "List private experiment IDs.", json!({})),
        ("writing_prepare_variant", "Preview a local prompt; never generate.", json!({"experiment":{"type":"string"},"variant":{"type":"string"}})),
        ("writing_list_candidates", "List immutable candidates, not manuscript drafts.", json!({"experiment":{"type":"string"}})),
        ("writing_read_candidate", "Read a candidate and hash-checked review history. Acceptance is not manuscript promotion.", json!({"experiment":{"type":"string"},"candidate":{"type":"string"}})),
        ("novelai_list_models", "Discover models with one authenticated GET; no generation.", json!({})),
    ];
    json!({"tools": definitions.into_iter().map(|(name, description, properties)| {
        let required: Vec<_> = properties.as_object().into_iter().flat_map(|p| p.keys().cloned()).collect();
        json!({"name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":name == "novelai_list_models"}})
    }).collect::<Vec<_>>()})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExperimentArg {
    experiment: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantArg {
    experiment: String,
    variant: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateArg {
    experiment: String,
    candidate: String,
}

fn error(code: i32, message: &str) -> Value {
    json!({"error":{"code":code,"message":message}})
}

fn call(
    store: &Store,
    params: &Value,
    models: &mut impl FnMut() -> Result<Vec<String>, ()>,
) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error(-32602, "Tool name required");
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Parse strictly before doing IO, including rejecting invented approval flags.
    let result: Result<Value, ()> = match name {
        "writing_list_experiments" | "novelai_list_models" => {
            if args.as_object().is_none_or(|object| !object.is_empty()) {
                return error(-32602, "Invalid tool arguments");
            }
            if name == "novelai_list_models" {
                models().map(|ids| json!(ids))
            } else {
                store.experiments().map(|ids| json!(ids)).map_err(|_| ())
            }
        }
        "writing_prepare_variant" => {
            let Ok(args) = serde_json::from_value::<VariantArg>(args) else {
                return error(-32602, "Invalid tool arguments");
            };
            store
                .experiment(&args.experiment)
                .and_then(|exp| exp.prepare(&args.variant))
                .map(|prompt| json!({"prompt":prompt.prompt()}))
                .map_err(|_| ())
        }
        "writing_list_candidates" => {
            let Ok(args) = serde_json::from_value::<ExperimentArg>(args) else {
                return error(-32602, "Invalid tool arguments");
            };
            store
                .candidates(&args.experiment)
                .map(|ids| json!(ids))
                .map_err(|_| ())
        }
        "writing_read_candidate" => {
            let Ok(args) = serde_json::from_value::<CandidateArg>(args) else {
                return error(-32602, "Invalid tool arguments");
            };
            store
                .read_candidate(&args.experiment, &args.candidate)
                .and_then(|text| {
                    store
                        .reviews(&args.experiment, &args.candidate)
                        .and_then(|reviews| {
                            if reviews
                                .iter()
                                .any(|review| review.candidate_hash != writing::hash(&text))
                            {
                                return Err(writing::Error::Invalid {
                                    reason: "candidate changed while reading reviews".into(),
                                });
                            }
                            Ok(json!({"text":text,"reviews":reviews}))
                        })
                })
                .map_err(|_| ())
        }
        _ => {
            return error(
                -32602,
                "Unknown tool; generation and mutations are CLI-only",
            )
        }
    };
    match result {
        Ok(value) => json!({"result":{"content":[{"type":"text","text":value.to_string()}]}}),
        Err(()) => {
            json!({"result":{"isError":true,"content":[{"type":"text","text":"Tool unavailable; check local files, 1Password, or API access. No generation was requested."}]}})
        }
    }
}

/// Protocol runner with an injectable read-only model source for deterministic
/// tests. Input is line-delimited JSON-RPC, never Content-Length framing.
pub fn run(
    store: &Store,
    mut input: impl BufRead,
    mut output: impl Write,
    mut models: impl FnMut() -> Result<Vec<String>, ()>,
) -> io::Result<()> {
    let mut initialized = false;
    loop {
        let mut line = Vec::new();
        let count = (&mut input).take(LIMIT + 1).read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        let reply = if count as u64 > LIMIT {
            if line.last() != Some(&b'\n') {
                input.skip_until(b'\n')?;
            }
            Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Request too large"}}),
            )
        } else {
            match serde_json::from_slice::<Value>(&line) {
                Err(_) => Some(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
                ),
                Ok(request) => {
                    let valid_id = request
                        .get("id")
                        .is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64());
                    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
                        || !request.get("method").is_some_and(Value::is_string)
                        || !valid_id
                    {
                        Some(
                            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}),
                        )
                    } else if request.get("id").is_none() {
                        None // Notifications, including initialized/cancelled, have no reply or side effects.
                    } else {
                        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
                        let method = request["method"].as_str().unwrap_or("");
                        let mut result = if !params.is_object() {
                            error(-32602, "Invalid params")
                        } else if method == "initialize" {
                            initialized = true;
                            let version = params
                                .get("protocolVersion")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let supported =
                                ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
                            json!({"result":{"protocolVersion":if supported.contains(&version) {version} else {"2025-11-25"},
                                "capabilities":{"tools":{}},"serverInfo":{"name":"blogctl-writing","version":env!("CARGO_PKG_VERSION")}}})
                        } else if method == "ping" {
                            json!({"result":{}})
                        } else if !initialized {
                            error(-32002, "Initialize first")
                        } else {
                            match method {
                                "tools/list" => json!({"result":tools()}),
                                "tools/call" => call(store, &params, &mut models),
                                _ => error(-32601, "Method not found"),
                            }
                        };
                        result["jsonrpc"] = json!("2.0");
                        result["id"] = request["id"].clone();
                        Some(result)
                    }
                }
            }
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
