use anyhow::{anyhow, Result};
use std::collections::HashMap;

use crate::api::{CreateTaskBody, Project, TaskListQuery, TodoistClient};

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

#[derive(Debug, Default, Clone)]
pub struct AddOpts {
    pub content: String,
    pub project: Option<String>,
    pub due: Option<String>,
    pub priority: Option<u8>,
    pub labels: Vec<String>,
    pub description: Option<String>,
}

pub fn run_add(
    client: &impl TodoistClient,
    token: &str,
    opts: &AddOpts,
) -> Result<serde_json::Value> {
    let project_id = match &opts.project {
        Some(arg) => {
            let projects = client.list_projects(token).map_err(|e| anyhow!(e))?;
            Some(resolve_project_id(&projects, arg)?)
        }
        None => None,
    };
    let body = CreateTaskBody {
        content: opts.content.clone(),
        project_id,
        due_string: opts.due.clone(),
        priority: opts.priority,
        labels: opts.labels.clone(),
        description: opts.description.clone(),
    };
    client.create_task(token, &body).map_err(|e| anyhow!(e))
}

pub fn render_added(task: &serde_json::Value) -> String {
    let id = task.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let prefix: String = id.chars().take(ID_PREFIX_LEN).collect();
    let content = task.get("content").and_then(|v| v.as_str()).unwrap_or("");
    format!("created {prefix} {content}")
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

pub struct CompletedTask {
    pub prefix: String,
    pub content: String,
}

pub struct CompleteResult {
    pub arg: String,
    pub outcome: Result<CompletedTask, String>,
}

pub fn run_complete(
    client: &impl TodoistClient,
    token: &str,
    args: &[String],
) -> Result<Vec<CompleteResult>> {
    let tasks = client
        .list_tasks(token, &TaskListQuery::default())
        .map_err(|e| anyhow!(e))?;
    let mut results = Vec::with_capacity(args.len());
    for arg in args {
        match resolve_target(&tasks, arg) {
            Ok(task) => {
                let id = task.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let content = task
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let prefix: String = id.chars().take(ID_PREFIX_LEN).collect();
                let outcome = match client.close_task(token, id) {
                    Ok(()) => Ok(CompletedTask { prefix, content }),
                    Err(e) => Err(e.to_string()),
                };
                results.push(CompleteResult {
                    arg: arg.clone(),
                    outcome,
                });
            }
            Err(e) => results.push(CompleteResult {
                arg: arg.clone(),
                outcome: Err(e),
            }),
        }
    }
    Ok(results)
}

pub fn resolve_target<'a>(
    tasks: &'a [serde_json::Value],
    arg: &str,
) -> Result<&'a serde_json::Value, String> {
    if let Some(exact) = tasks
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(arg))
    {
        return Ok(exact);
    }
    let prefix_matches: Vec<&serde_json::Value> = tasks
        .iter()
        .filter(|t| {
            t.get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.starts_with(arg))
                .unwrap_or(false)
        })
        .collect();
    match prefix_matches.as_slice() {
        [] => Err(format!("no task matching {arg:?}")),
        [hit] => Ok(*hit),
        many => {
            let candidates: Vec<String> = many
                .iter()
                .map(|t| {
                    let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let prefix: String = id.chars().take(ID_PREFIX_LEN).collect();
                    let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    format!("{prefix} ({content})")
                })
                .collect();
            Err(format!(
                "ambiguous prefix {arg:?}: matches {}",
                candidates.join(", ")
            ))
        }
    }
}

pub fn render_complete_results(results: &[CompleteResult], use_unicode: bool) -> (String, bool) {
    let ok_mark = if use_unicode { "✓" } else { "ok" };
    let err_mark = if use_unicode { "✗" } else { "x" };
    let mut any_failure = false;
    let lines: Vec<String> = results
        .iter()
        .map(|r| match &r.outcome {
            Ok(task) => format!("{ok_mark} {} {}", task.prefix, task.content),
            Err(e) => {
                any_failure = true;
                format!("{err_mark} {} ({e})", r.arg)
            }
        })
        .collect();
    (lines.join("\n"), any_failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApiError, CreateTaskBody, Project, TaskListQuery, TodoistClient};
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FakeClient {
        projects: Vec<Project>,
        tasks: Vec<serde_json::Value>,
        last_query: RefCell<Option<TaskListQuery>>,
        last_create: RefCell<Option<CreateTaskBody>>,
        create_response: serde_json::Value,
        create_error: RefCell<Option<ApiError>>,
        closed_ids: RefCell<Vec<String>>,
        close_failure_ids: Vec<String>,
    }

    impl FakeClient {
        fn new(projects: Vec<Project>, tasks: Vec<serde_json::Value>) -> Self {
            Self {
                projects,
                tasks,
                last_query: RefCell::new(None),
                last_create: RefCell::new(None),
                create_response: serde_json::json!({"id": "999abc12345", "content": "echo"}),
                create_error: RefCell::new(None),
                closed_ids: RefCell::new(Vec::new()),
                close_failure_ids: Vec::new(),
            }
        }

        fn with_create_error(mut self, err: ApiError) -> Self {
            self.create_error = RefCell::new(Some(err));
            self
        }

        fn with_close_failure(mut self, id: &str) -> Self {
            self.close_failure_ids.push(id.to_string());
            self
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

        fn create_task(
            &self,
            _token: &str,
            body: &CreateTaskBody,
        ) -> Result<serde_json::Value, ApiError> {
            *self.last_create.borrow_mut() = Some(body.clone());
            if let Some(e) = self.create_error.borrow_mut().take() {
                return Err(e);
            }
            Ok(self.create_response.clone())
        }

        fn close_task(&self, _token: &str, id: &str) -> Result<(), ApiError> {
            if self.close_failure_ids.iter().any(|f| f == id) {
                return Err(ApiError::Other {
                    status: 404,
                    body: format!("task {id} not found"),
                });
            }
            self.closed_ids.borrow_mut().push(id.to_string());
            Ok(())
        }
    }

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: id.to_string(),
            name: name.to_string(),
            parent_id: None,
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

    #[test]
    fn run_add_forwards_all_fields_to_create_body() {
        let client = FakeClient::new(vec![project("9", "Errands")], vec![]);
        let opts = AddOpts {
            content: "buy milk".into(),
            project: Some("Errands".into()),
            due: Some("tomorrow".into()),
            priority: Some(3),
            labels: vec!["shopping".into()],
            description: Some("organic".into()),
        };
        run_add(&client, "tok", &opts).unwrap();
        let body = client.last_create.borrow().clone().unwrap();
        assert_eq!(body.content, "buy milk");
        assert_eq!(body.project_id.as_deref(), Some("9"));
        assert_eq!(body.due_string.as_deref(), Some("tomorrow"));
        assert_eq!(body.priority, Some(3));
        assert_eq!(body.labels, vec!["shopping"]);
        assert_eq!(body.description.as_deref(), Some("organic"));
    }

    #[test]
    fn run_add_skips_project_lookup_when_unset() {
        let client = FakeClient::new(vec![], vec![]);
        let opts = AddOpts {
            content: "x".into(),
            ..Default::default()
        };
        run_add(&client, "tok", &opts).unwrap();
        let body = client.last_create.borrow().clone().unwrap();
        assert!(body.project_id.is_none());
        assert!(body.labels.is_empty());
    }

    #[test]
    fn run_add_surfaces_create_error() {
        let client = FakeClient::new(vec![], vec![]).with_create_error(ApiError::Other {
            status: 400,
            body: "Bad content".into(),
        });
        let opts = AddOpts {
            content: "x".into(),
            ..Default::default()
        };
        let err = run_add(&client, "tok", &opts).unwrap_err();
        assert!(err.to_string().contains("400"));
        assert!(err.to_string().contains("Bad content"));
    }

    #[test]
    fn render_added_uses_id_prefix_and_content() {
        let task = serde_json::json!({"id": "abcdef9876543210", "content": "buy milk"});
        let out = render_added(&task);
        assert!(out.contains("created"));
        assert!(out.contains("abcdef"));
        assert!(!out.contains("9876543210"));
        assert!(out.contains("buy milk"));
    }

    #[test]
    fn resolve_target_matches_exact_id() {
        let tasks = vec![task("abcdef0000", "x", "1", 1, None)];
        let hit = resolve_target(&tasks, "abcdef0000").unwrap();
        assert_eq!(hit.get("id").and_then(|v| v.as_str()), Some("abcdef0000"));
    }

    #[test]
    fn resolve_target_matches_unique_prefix() {
        let tasks = vec![
            task("abcdef0000", "x", "1", 1, None),
            task("ghijkl1111", "y", "1", 1, None),
        ];
        let hit = resolve_target(&tasks, "abc").unwrap();
        assert_eq!(hit.get("id").and_then(|v| v.as_str()), Some("abcdef0000"));
    }

    #[test]
    fn resolve_target_errors_on_ambiguous_prefix() {
        let tasks = vec![
            task("abc111", "first", "1", 1, None),
            task("abc222", "second", "1", 1, None),
        ];
        let err = resolve_target(&tasks, "abc").unwrap_err();
        assert!(err.contains("ambiguous"));
        assert!(err.contains("first"));
        assert!(err.contains("second"));
    }

    #[test]
    fn resolve_target_errors_when_no_match() {
        let tasks = vec![task("aaa", "x", "1", 1, None)];
        let err = resolve_target(&tasks, "zzz").unwrap_err();
        assert!(err.contains("no task matching"));
    }

    #[test]
    fn run_complete_closes_resolved_ids() {
        let tasks = vec![
            task("abcdef1111", "first", "1", 1, None),
            task("ghijkl2222", "second", "1", 1, None),
        ];
        let client = FakeClient::new(vec![], tasks);
        let args = vec!["abc".to_string(), "ghijkl2222".to_string()];
        let results = run_complete(&client, "tok", &args).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].outcome.is_ok());
        assert!(results[1].outcome.is_ok());
        let closed = client.closed_ids.borrow();
        assert_eq!(*closed, vec!["abcdef1111", "ghijkl2222"]);
    }

    #[test]
    fn run_complete_reports_resolve_failure_but_continues() {
        let tasks = vec![task("aaa111", "real", "1", 1, None)];
        let client = FakeClient::new(vec![], tasks);
        let args = vec!["zzz".to_string(), "aaa".to_string()];
        let results = run_complete(&client, "tok", &args).unwrap();
        assert!(results[0].outcome.is_err());
        assert!(results[1].outcome.is_ok());
        assert_eq!(*client.closed_ids.borrow(), vec!["aaa111"]);
    }

    #[test]
    fn run_complete_reports_api_failure_but_continues() {
        let tasks = vec![
            task("aaa111", "first", "1", 1, None),
            task("bbb222", "second", "1", 1, None),
        ];
        let client = FakeClient::new(vec![], tasks).with_close_failure("aaa111");
        let args = vec!["aaa".to_string(), "bbb".to_string()];
        let results = run_complete(&client, "tok", &args).unwrap();
        assert!(results[0].outcome.is_err());
        assert!(results[1].outcome.is_ok());
        assert_eq!(*client.closed_ids.borrow(), vec!["bbb222"]);
    }

    #[test]
    fn render_complete_results_uses_unicode_when_enabled() {
        let results = vec![CompleteResult {
            arg: "abc".into(),
            outcome: Ok(CompletedTask {
                prefix: "abcdef".into(),
                content: "buy milk".into(),
            }),
        }];
        let (out, failed) = render_complete_results(&results, true);
        assert!(out.contains("✓"));
        assert!(out.contains("abcdef"));
        assert!(out.contains("buy milk"));
        assert!(!failed);
    }

    #[test]
    fn render_complete_results_uses_ascii_when_disabled() {
        let results = vec![CompleteResult {
            arg: "abc".into(),
            outcome: Ok(CompletedTask {
                prefix: "abcdef".into(),
                content: "x".into(),
            }),
        }];
        let (out, _) = render_complete_results(&results, false);
        assert!(out.contains("ok"));
        assert!(!out.contains("✓"));
    }

    #[test]
    fn render_complete_results_flags_any_failure() {
        let results = vec![
            CompleteResult {
                arg: "abc".into(),
                outcome: Ok(CompletedTask {
                    prefix: "abcdef".into(),
                    content: "x".into(),
                }),
            },
            CompleteResult {
                arg: "zzz".into(),
                outcome: Err("no match".into()),
            },
        ];
        let (out, failed) = render_complete_results(&results, true);
        assert!(failed);
        assert!(out.contains("zzz"));
        assert!(out.contains("no match"));
    }
}
