use std::io;
use std::process::Command;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum TodoistError {
    #[snafu(display("could not spawn `todoist`: {source} (is `todoist` on PATH?)"))]
    Spawn { source: io::Error },

    #[snafu(display("`todoist projects list --json` exited {status}: {stderr}"))]
    NonZeroExit { status: i32, stderr: String },

    #[snafu(display(
        "failed to parse line {line_no} from `todoist projects list --json`: {source}"
    ))]
    ParseJson {
        line_no: usize,
        source: serde_json::Error,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Invoke `todoist projects list --json` and parse the ndjson output.
/// `todoist` is expected to be on `PATH` at runtime — it's the sibling
/// crate `tools/todoist`; `aof` deliberately doesn't carry its own
/// Todoist API client.
pub fn list_projects() -> Result<Vec<Project>, TodoistError> {
    let output = Command::new("todoist")
        .args(["projects", "list", "--json"])
        .output()
        .context(SpawnSnafu)?;
    if !output.status.success() {
        return NonZeroExitSnafu {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
        .fail();
    }
    parse_ndjson(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ndjson(s: &str) -> Result<Vec<Project>, TodoistError> {
    s.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).context(ParseJsonSnafu { line_no: i + 1 }))
        .collect()
}

pub fn render_table(projects: &[Project]) -> String {
    if projects.is_empty() {
        return "no projects".to_string();
    }
    projects
        .iter()
        .map(|p| match &p.parent_id {
            Some(parent) => format!("{}  {}  [parent={parent}]", p.id, p.name),
            None => format!("{}  {}", p.id, p.name),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_ndjson(projects: &[Project]) -> String {
    projects
        .iter()
        .map(|p| serde_json::to_string(p).expect("Project always serializes"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ndjson_handles_valid_lines() {
        let raw = r#"{"id":"1","name":"Inbox","parent_id":null}
{"id":"2","name":"Errands","parent_id":"1"}"#;
        let projects = parse_ndjson(raw).expect("parse should succeed");
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "Inbox");
        assert_eq!(projects[0].parent_id, None);
        assert_eq!(projects[1].name, "Errands");
        assert_eq!(projects[1].parent_id.as_deref(), Some("1"));
    }

    #[test]
    fn parse_ndjson_handles_empty_input() {
        let projects = parse_ndjson("").expect("empty input is valid");
        assert!(projects.is_empty());
    }

    #[test]
    fn parse_ndjson_skips_blank_lines() {
        // Trailing newline is common from line-oriented output; blank
        // separator lines shouldn't be treated as parse failures either.
        let raw = "\n{\"id\":\"1\",\"name\":\"Inbox\",\"parent_id\":null}\n\n";
        let projects = parse_ndjson(raw).expect("blanks should be skipped");
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn parse_ndjson_reports_line_number_on_bad_input() {
        // Line 2 is malformed; the error should point at line 2 so a
        // user piping through tools can find the bad row.
        let raw = "{\"id\":\"1\",\"name\":\"a\",\"parent_id\":null}\nnot json";
        let err = parse_ndjson(raw).expect_err("garbage line must error");
        let msg = err.to_string();
        assert!(msg.contains("line 2"), "got: {msg}");
    }

    #[test]
    fn project_treats_missing_parent_id_as_none() {
        // todoist's render_ndjson always emits `parent_id` (null or
        // string); but tolerating a missing key keeps us robust if the
        // upstream shape ever drifts.
        let raw = r#"{"id":"1","name":"Inbox"}"#;
        let parsed: Project = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.parent_id, None);
    }

    #[test]
    fn render_table_shows_one_project_per_line() {
        let projects = vec![
            Project {
                id: "1".into(),
                name: "Inbox".into(),
                parent_id: None,
            },
            Project {
                id: "2".into(),
                name: "Errands".into(),
                parent_id: Some("1".into()),
            },
        ];
        let out = render_table(&projects);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Inbox"));
        assert!(!lines[0].contains("parent="));
        assert!(lines[1].contains("Errands"));
        assert!(lines[1].contains("parent=1"));
    }

    #[test]
    fn render_table_reports_empty_set() {
        assert_eq!(render_table(&[]), "no projects");
    }

    #[test]
    fn render_ndjson_each_line_parses_back() {
        let projects = vec![
            Project {
                id: "1".into(),
                name: "Inbox".into(),
                parent_id: None,
            },
            Project {
                id: "2".into(),
                name: "Errands".into(),
                parent_id: Some("1".into()),
            },
        ];
        let out = render_ndjson(&projects);
        // Round-trip: parse our own output back into Vec<Project>.
        let round_trip = parse_ndjson(&out).expect("our ndjson should re-parse");
        assert_eq!(round_trip, projects);
    }
}
