use anyhow::{anyhow, Result};
use std::collections::HashMap;

use crate::api::{Project, TaskListQuery, TodoistClient};

pub const ID_PREFIX_LEN: usize = 6;

#[derive(Debug, Clone)]
pub struct ListOpts {
    pub project: Option<String>,
    pub filter: Option<String>,
    pub limit: Option<usize>,
}

pub struct ListOutcome {
    pub tasks: Vec<serde_json::Value>,
    pub projects_by_id: HashMap<String, String>,
}

pub fn run_list(client: &impl TodoistClient, token: &str, opts: &ListOpts) -> Result<ListOutcome> {
    let projects = client.list_projects(token).map_err(|e| anyhow!(e))?;
    let project_id = match &opts.project {
        Some(arg) => Some(resolve_project_id(&projects, arg)?),
        None => None,
    };
    let query = TaskListQuery {
        project_id,
        filter: opts.filter.clone(),
    };
    let mut tasks = client.list_tasks(token, &query).map_err(|e| anyhow!(e))?;
    if let Some(n) = opts.limit {
        tasks.truncate(n);
    }
    let projects_by_id = projects.into_iter().map(|p| (p.id, p.name)).collect();
    Ok(ListOutcome {
        tasks,
        projects_by_id,
    })
}

pub fn resolve_project_id(projects: &[Project], arg: &str) -> Result<String> {
    if arg.chars().all(|c| c.is_ascii_digit()) {
        return Ok(arg.to_string());
    }
    let matches: Vec<&Project> = projects.iter().filter(|p| p.name == arg).collect();
    match matches.as_slice() {
        [] => {
            let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
            Err(anyhow!(
                "no project named {arg:?} (known: {})",
                names.join(", ")
            ))
        }
        [hit] => Ok(hit.id.clone()),
        many => Err(anyhow!(
            "{} projects named {arg:?} — disambiguate by id",
            many.len()
        )),
    }
}

pub fn render_ndjson(tasks: &[serde_json::Value]) -> String {
    tasks
        .iter()
        .map(|t| serde_json::to_string(t).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_table(
    tasks: &[serde_json::Value],
    projects_by_id: &HashMap<String, String>,
) -> String {
    if tasks.is_empty() {
        return "no tasks".to_string();
    }
    let rows: Vec<[String; 5]> = tasks
        .iter()
        .map(|t| {
            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let id_short = id.chars().take(ID_PREFIX_LEN).collect::<String>();
            let content = t
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let project_id = t.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let project = projects_by_id
                .get(project_id)
                .cloned()
                .unwrap_or_else(|| project_id.to_string());
            let due = t
                .get("due")
                .and_then(|d| d.get("string"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let priority = t
                .get("priority")
                .and_then(|v| v.as_u64())
                .map(|p| p.to_string())
                .unwrap_or_default();
            [id_short, content, project, due, priority]
        })
        .collect();

    let headers = ["id", "content", "project", "due", "priority"];
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let format_row = |cells: &[String; 5]| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:width$}", c, width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
    };

    let header_row = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
        .collect::<Vec<_>>()
        .join("  ");

    let mut lines = vec![header_row];
    lines.extend(rows.iter().map(format_row));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApiError, Project, TaskListQuery, TodoistClient};
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FakeClient {
        projects: Vec<Project>,
        tasks: Vec<serde_json::Value>,
        last_query: RefCell<Option<TaskListQuery>>,
    }

    impl FakeClient {
        fn new(projects: Vec<Project>, tasks: Vec<serde_json::Value>) -> Self {
            Self {
                projects,
                tasks,
                last_query: RefCell::new(None),
            }
        }
    }

    impl TodoistClient for FakeClient {
        fn list_tasks(
            &self,
            _token: &str,
            query: &TaskListQuery,
        ) -> Result<Vec<serde_json::Value>, ApiError> {
            *self.last_query.borrow_mut() = Some(query.clone());
            Ok(self.tasks.clone())
        }

        fn list_projects(&self, _token: &str) -> Result<Vec<Project>, ApiError> {
            Ok(self.projects.clone())
        }
    }

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    fn task(
        id: &str,
        content: &str,
        project_id: &str,
        priority: u64,
        due: Option<&str>,
    ) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "id": id,
            "content": content,
            "project_id": project_id,
            "priority": priority,
        });
        if let Some(d) = due {
            obj["due"] = serde_json::json!({ "string": d, "date": "2026-05-25" });
        }
        obj
    }

    #[test]
    fn resolve_project_id_passes_numeric_through() {
        let id = resolve_project_id(&[], "12345").unwrap();
        assert_eq!(id, "12345");
    }

    #[test]
    fn resolve_project_id_finds_exact_name_match() {
        let projects = vec![project("1", "Inbox"), project("2", "Errands")];
        let id = resolve_project_id(&projects, "Errands").unwrap();
        assert_eq!(id, "2");
    }

    #[test]
    fn resolve_project_id_errors_on_unknown_name() {
        let projects = vec![project("1", "Inbox")];
        let err = resolve_project_id(&projects, "Missing").unwrap_err();
        assert!(err.to_string().contains("no project named"));
        assert!(err.to_string().contains("Inbox"));
    }

    #[test]
    fn resolve_project_id_errors_on_ambiguous_name() {
        let projects = vec![project("1", "Dup"), project("2", "Dup")];
        let err = resolve_project_id(&projects, "Dup").unwrap_err();
        assert!(err.to_string().contains("disambiguate"));
    }

    #[test]
    fn run_list_forwards_project_name_to_query_as_id() {
        let client = FakeClient::new(vec![project("9", "Errands")], vec![]);
        let opts = ListOpts {
            project: Some("Errands".into()),
            filter: None,
            limit: None,
        };
        run_list(&client, "tok", &opts).unwrap();
        let q = client.last_query.borrow();
        assert_eq!(q.as_ref().unwrap().project_id.as_deref(), Some("9"));
    }

    #[test]
    fn run_list_forwards_filter_unchanged() {
        let client = FakeClient::new(vec![], vec![]);
        let opts = ListOpts {
            project: None,
            filter: Some("today".into()),
            limit: None,
        };
        run_list(&client, "tok", &opts).unwrap();
        assert_eq!(
            client
                .last_query
                .borrow()
                .as_ref()
                .unwrap()
                .filter
                .as_deref(),
            Some("today")
        );
    }

    #[test]
    fn run_list_truncates_to_limit() {
        let tasks = vec![task("a", "1", "1", 1, None), task("b", "2", "1", 1, None)];
        let client = FakeClient::new(vec![], tasks);
        let opts = ListOpts {
            project: None,
            filter: None,
            limit: Some(1),
        };
        let outcome = run_list(&client, "tok", &opts).unwrap();
        assert_eq!(outcome.tasks.len(), 1);
    }

    #[test]
    fn render_ndjson_emits_one_object_per_line() {
        let tasks = vec![
            task("a", "first", "1", 1, None),
            task("b", "second", "1", 1, None),
        ];
        let out = render_ndjson(&tasks);
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("\"content\":\"first\""));
        assert!(out.contains("\"content\":\"second\""));
    }

    #[test]
    fn render_table_truncates_id_and_shows_project_name() {
        let tasks = vec![task("abcdef1234567890", "buy milk", "42", 3, Some("today"))];
        let mut projects_by_id = HashMap::new();
        projects_by_id.insert("42".into(), "Errands".into());
        let out = render_table(&tasks, &projects_by_id);
        assert!(out.contains("abcdef"));
        assert!(!out.contains("1234567890"));
        assert!(out.contains("buy milk"));
        assert!(out.contains("Errands"));
        assert!(out.contains("today"));
    }

    #[test]
    fn render_table_falls_back_to_id_when_project_unknown() {
        let tasks = vec![task("x", "y", "99", 1, None)];
        let out = render_table(&tasks, &HashMap::new());
        assert!(out.contains("99"));
    }

    #[test]
    fn render_table_reports_empty_set() {
        let out = render_table(&[], &HashMap::new());
        assert_eq!(out, "no tasks");
    }
}
