//! Merge only the owned writing server into personal client configuration.
//! No credential resolution or full-file declarative ownership is involved.
use std::fs::{self, Permissions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

use serde_json::{json, Value};
use tempfile::NamedTempFile;
use toml_edit::{value, DocumentMut, Item, Table};

use crate::writing::{self, read_text};

fn invalid() -> writing::Error {
    writing::Error::Invalid { reason: "MCP configuration conflict, symlink, or invalid format; inspect configuration locally (contents suppressed)".into() }
}

fn owned(command: &str, executable: &str) -> bool {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        == Some(executable)
        && Path::new(command).is_absolute()
}

fn legacy(command: &str, args: &Value) -> bool {
    command.starts_with("/nix/store/") && owned(command, "novelai-lab") && *args == json!(["mcp"])
}

fn source(path: &Path) -> writing::Result<String> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_text(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(writing::Error::Io {
            path: path.into(),
            source,
        }),
    }
}

fn atomic(path: &Path, before: &str, after: &str) -> writing::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|source| writing::Error::Io {
            path: parent.into(),
            source,
        })?;
    if source(path)? != before {
        return Err(invalid());
    }
    let mut file = NamedTempFile::new_in(parent).map_err(|source| writing::Error::Io {
        path: parent.into(),
        source,
    })?;
    file.write_all(after.as_bytes())
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    file.as_file()
        .set_permissions(Permissions::from_mode(0o600))
        .and_then(|()| file.as_file().sync_all())
        .map_err(|source| writing::Error::Io {
            path: path.into(),
            source,
        })?;
    file.persist(path).map_err(|err| writing::Error::Io {
        path: path.into(),
        source: err.error,
    })?;
    Ok(())
}

pub fn install(
    home: &Path,
    command: &Path,
    workdir: &Path,
    models_command: Option<&Path>,
) -> writing::Result<()> {
    let command = command.to_str().ok_or_else(invalid)?;
    if !owned(command, "blogctl") || !workdir.is_absolute() {
        return Err(invalid());
    }
    let mut args = vec![
        "mcp".to_owned(),
        "serve".into(),
        "--workdir".into(),
        workdir.to_str().ok_or_else(invalid)?.into(),
    ];
    if let Some(models) = models_command {
        if !models.is_absolute() {
            return Err(invalid());
        }
        args.extend([
            "--models-command".into(),
            models.to_str().ok_or_else(invalid)?.into(),
        ]);
    }
    let pi_path = home.join(".pi/agent/mcp.json");
    let codex_path = home.join(".codex/config.toml");
    let pi_before = source(&pi_path)?;
    let codex_before = source(&codex_path)?;
    let mut pi: Value = if pi_before.is_empty() {
        json!({})
    } else {
        serde_json::from_str(&pi_before).map_err(|_| invalid())?
    };
    let root = pi.as_object_mut().ok_or_else(invalid)?;
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    if let Some(entry) = servers.get("writing") {
        if !entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| owned(c, "blogctl"))
            || entry.get("url").is_some()
            || entry
                .get("args")
                .and_then(Value::as_array)
                .is_none_or(|args| {
                    args.first().and_then(Value::as_str) != Some("mcp")
                        || args.get(1).and_then(Value::as_str) != Some("serve")
                })
        {
            return Err(invalid());
        }
    }
    if servers
        .get("novelai")
        .is_some_and(|entry| legacy(entry["command"].as_str().unwrap_or(""), &entry["args"]))
    {
        servers.remove("novelai");
    }
    let entry = servers
        .entry("writing")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    entry.insert("command".into(), json!(command));
    entry.insert("args".into(), json!(args));
    entry.entry("exposure").or_insert_with(|| json!("direct"));
    let pi_after = serde_json::to_string_pretty(&pi).map_err(|_| invalid())? + "\n";

    let mut codex: DocumentMut = codex_before.parse().map_err(|_| invalid())?;
    if codex.get("mcp_servers").is_none() {
        codex["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = codex["mcp_servers"].as_table_mut().ok_or_else(invalid)?;
    if let Some(entry) = servers.get("writing") {
        if !entry
            .get("command")
            .and_then(Item::as_str)
            .is_some_and(|c| owned(c, "blogctl"))
            || entry.get("url").is_some()
            || entry
                .get("args")
                .and_then(Item::as_array)
                .is_none_or(|args| {
                    args.get(0).and_then(toml_edit::Value::as_str) != Some("mcp")
                        || args.get(1).and_then(toml_edit::Value::as_str) != Some("serve")
                })
        {
            return Err(invalid());
        }
    }
    let remove_old = servers.get("novelai").is_some_and(|entry| {
        let command = entry.get("command").and_then(Item::as_str).unwrap_or("");
        entry
            .get("args")
            .and_then(Item::as_array)
            .and_then(|array| {
                array
                    .iter()
                    .map(toml_edit::Value::as_str)
                    .collect::<Option<Vec<_>>>()
            })
            .is_some_and(|args| legacy(command, &json!(args)))
    });
    if remove_old {
        servers.remove("novelai");
    }
    if servers.get("writing").is_none() {
        servers.insert("writing", Item::Table(Table::new()));
    }
    let writing = servers
        .get_mut("writing")
        .and_then(Item::as_table_mut)
        .ok_or_else(invalid)?;
    writing.insert("command", value(command));
    let mut array = toml_edit::Array::new();
    for arg in &args {
        array.push(arg.as_str());
    }
    writing.insert("args", value(array));
    let codex_after = codex.to_string();
    // Validate both complete documents before modifying either file. Each
    // replacement is atomic; rerunning safely repairs a partial two-file update.
    if pi_before != pi_after {
        atomic(&pi_path, &pi_before, &pi_after)?;
    }
    if codex_before != codex_after {
        atomic(&codex_path, &codex_before, &codex_after)?;
    }
    Ok(())
}
