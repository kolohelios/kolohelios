use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub token_op_ref: String,
}

#[derive(Debug, PartialEq, Eq, Snafu)]
#[snafu(display("{message}"))]
pub struct OpError {
    pub message: String,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum AuthError {
    #[snafu(display("HOME is not set"))]
    HomeNotSet { source: std::env::VarError },

    #[snafu(display("could not resolve {op_ref}: {source}"))]
    OpResolve { op_ref: String, source: OpError },

    #[snafu(display("serializing config"))]
    TomlSerialize { source: toml::ser::Error },

    #[snafu(display("parsing config"))]
    TomlParse { source: toml::de::Error },

    #[snafu(display("creating {}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("writing {}", path.display()))]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("reading {}", path.display()))]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("removing {}", path.display()))]
    Remove {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub trait OpRunner {
    fn read(&self, op_ref: &str) -> Result<String, OpError>;
}

pub struct RealOp;

impl OpRunner for RealOp {
    fn read(&self, op_ref: &str) -> Result<String, OpError> {
        let output = Command::new("op")
            .args(["read", op_ref])
            .output()
            .map_err(|e| OpError {
                message: format!("could not run `op`: {e}"),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(OpError {
                message: format!("op read failed: {stderr}"),
            });
        }
        // SECURITY: never log or debug-print this string. It is the
        // Todoist API token in plaintext, lifted from 1Password just
        // long enough to make the request.
        let token = String::from_utf8(output.stdout)
            .map_err(|e| OpError {
                message: format!("op read stdout not utf8: {e}"),
            })?
            .trim_end_matches('\n')
            .to_string();
        Ok(token)
    }
}

pub fn default_config_path() -> Result<PathBuf, AuthError> {
    let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME").context(HomeNotSetSnafu)?;
        PathBuf::from(home).join(".config")
    };
    Ok(base.join("todoist").join("config.toml"))
}

pub fn login(op: &impl OpRunner, config_path: &Path, op_ref: &str) -> Result<(), AuthError> {
    op.read(op_ref).context(OpResolveSnafu {
        op_ref: op_ref.to_string(),
    })?;
    let config = Config {
        token_op_ref: op_ref.to_string(),
    };
    let serialized = toml::to_string(&config).context(TomlSerializeSnafu)?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).context(CreateDirSnafu {
            path: parent.to_path_buf(),
        })?;
    }
    std::fs::write(config_path, serialized).context(WriteSnafu {
        path: config_path.to_path_buf(),
    })?;
    Ok(())
}

pub enum AuthState {
    NotLoggedIn,
    Configured {
        op_ref: String,
        resolve_err: Option<String>,
    },
}

pub fn status(op: &impl OpRunner, config_path: &Path) -> Result<AuthState, AuthError> {
    if !config_path.exists() {
        return Ok(AuthState::NotLoggedIn);
    }
    let raw = std::fs::read_to_string(config_path).context(ReadSnafu {
        path: config_path.to_path_buf(),
    })?;
    let config: Config = toml::from_str(&raw).context(TomlParseSnafu)?;
    let resolve_err = op.read(&config.token_op_ref).err().map(|e| e.message);
    Ok(AuthState::Configured {
        op_ref: config.token_op_ref,
        resolve_err,
    })
}

pub fn logout(config_path: &Path) -> Result<(), AuthError> {
    match std::fs::remove_file(config_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context(RemoveSnafu {
            path: config_path.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct FakeOp {
        responses: Mutex<HashMap<String, Result<String, String>>>,
    }

    impl FakeOp {
        fn ok(op_ref: &str, value: &str) -> Self {
            let mut m = HashMap::new();
            m.insert(op_ref.to_string(), Ok(value.to_string()));
            Self {
                responses: Mutex::new(m),
            }
        }

        fn err(op_ref: &str, message: &str) -> Self {
            let mut m = HashMap::new();
            m.insert(op_ref.to_string(), Err(message.to_string()));
            Self {
                responses: Mutex::new(m),
            }
        }
    }

    impl OpRunner for FakeOp {
        fn read(&self, op_ref: &str) -> Result<String, OpError> {
            let m = self.responses.lock().unwrap();
            match m.get(op_ref) {
                Some(Ok(v)) => Ok(v.clone()),
                Some(Err(msg)) => Err(OpError {
                    message: msg.clone(),
                }),
                None => Err(OpError {
                    message: format!("unexpected ref {op_ref}"),
                }),
            }
        }
    }

    fn temp_path() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        (dir, path)
    }

    #[test]
    fn config_round_trips_through_toml() {
        let config = Config {
            token_op_ref: "op://Personal/Todoist API/credential".to_string(),
        };
        let serialized = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn login_writes_config_when_op_resolves() {
        let (_dir, path) = temp_path();
        let op = FakeOp::ok("op://Personal/Todoist/credential", "the-token");
        login(&op, &path, "op://Personal/Todoist/credential").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("op://Personal/Todoist/credential"));
        assert!(!raw.contains("the-token"), "token must never hit disk");
    }

    #[test]
    fn login_does_not_write_when_op_errors() {
        let (_dir, path) = temp_path();
        let op = FakeOp::err("op://Personal/Todoist/credential", "vault locked");
        let err = login(&op, &path, "op://Personal/Todoist/credential").unwrap_err();
        assert!(err.to_string().contains("vault locked"));
        assert!(!path.exists(), "config must not be written on failure");
    }

    #[test]
    fn status_reports_not_logged_in_when_config_missing() {
        let (_dir, path) = temp_path();
        let op = FakeOp::ok("anything", "x");
        matches!(status(&op, &path).unwrap(), AuthState::NotLoggedIn);
    }

    #[test]
    fn status_reports_configured_and_ok_when_resolve_succeeds() {
        let (_dir, path) = temp_path();
        let op = FakeOp::ok("op://x", "tok");
        login(&op, &path, "op://x").unwrap();
        match status(&op, &path).unwrap() {
            AuthState::Configured {
                op_ref,
                resolve_err,
            } => {
                assert_eq!(op_ref, "op://x");
                assert!(resolve_err.is_none());
            }
            _ => panic!("expected Configured"),
        }
    }

    #[test]
    fn status_reports_configured_with_error_when_resolve_fails() {
        let (_dir, path) = temp_path();
        let writable = FakeOp::ok("op://x", "tok");
        login(&writable, &path, "op://x").unwrap();
        let broken = FakeOp::err("op://x", "vault locked");
        match status(&broken, &path).unwrap() {
            AuthState::Configured {
                op_ref,
                resolve_err,
            } => {
                assert_eq!(op_ref, "op://x");
                assert_eq!(resolve_err.as_deref(), Some("vault locked"));
            }
            _ => panic!("expected Configured"),
        }
    }

    #[test]
    fn logout_removes_existing_config() {
        let (_dir, path) = temp_path();
        let op = FakeOp::ok("op://x", "tok");
        login(&op, &path, "op://x").unwrap();
        assert!(path.exists());
        logout(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn logout_is_idempotent_when_no_config() {
        let (_dir, path) = temp_path();
        logout(&path).unwrap();
    }
}
