use anyhow::{anyhow, Result};

use crate::api::{Project, TodoistClient};

pub fn run_list(client: &impl TodoistClient, token: &str) -> Result<Vec<Project>> {
    client.list_projects(token).map_err(|e| anyhow!(e))
}

pub fn render_ndjson(projects: &[Project]) -> String {
    projects
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "parent_id": p.parent_id,
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_table(projects: &[Project]) -> String {
    if projects.is_empty() {
        return "no projects".to_string();
    }
    let rows: Vec<[String; 3]> = projects
        .iter()
        .map(|p| {
            [
                p.name.clone(),
                p.id.clone(),
                p.parent_id.clone().unwrap_or_default(),
            ]
        })
        .collect();

    let headers = ["name", "id", "parent_id"];
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let format_row = |cells: &[String; 3]| -> String {
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
    use crate::api::{ApiError, CreateTaskBody, Project, TaskListQuery, TodoistClient};

    struct FakeClient {
        projects: Vec<Project>,
    }

    impl TodoistClient for FakeClient {
        fn list_tasks(
            &self,
            _token: &str,
            _query: &TaskListQuery,
        ) -> Result<Vec<serde_json::Value>, ApiError> {
            Ok(vec![])
        }

        fn list_projects(&self, _token: &str) -> Result<Vec<Project>, ApiError> {
            Ok(self.projects.clone())
        }

        fn create_task(
            &self,
            _token: &str,
            _body: &CreateTaskBody,
        ) -> Result<serde_json::Value, ApiError> {
            unimplemented!()
        }

        fn close_task(&self, _token: &str, _id: &str) -> Result<(), ApiError> {
            unimplemented!()
        }
    }

    fn project(id: &str, name: &str, parent_id: Option<&str>) -> Project {
        Project {
            id: id.to_string(),
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
        }
    }

    #[test]
    fn run_list_returns_projects_from_client() {
        let client = FakeClient {
            projects: vec![
                project("1", "Inbox", None),
                project("2", "Errands", Some("1")),
            ],
        };
        let out = run_list(&client, "tok").unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].parent_id.as_deref(), Some("1"));
    }

    #[test]
    fn render_table_reports_empty_set() {
        assert_eq!(render_table(&[]), "no projects");
    }

    #[test]
    fn render_table_shows_name_id_and_parent_id() {
        let projects = vec![
            project("1", "Inbox", None),
            project("22", "Errands", Some("1")),
        ];
        let out = render_table(&projects);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].contains("name"));
        assert!(lines[0].contains("id"));
        assert!(lines[0].contains("parent_id"));
        assert!(out.contains("Inbox"));
        assert!(out.contains("Errands"));
        assert!(out.contains("22"));
    }

    #[test]
    fn render_table_leaves_parent_id_blank_for_top_level() {
        let projects = vec![project("1", "Top", None)];
        let out = render_table(&projects);
        let data_row = out.lines().nth(1).unwrap();
        // No trailing token after the id column means parent_id rendered empty.
        assert!(data_row.trim_end().ends_with('1'));
    }

    #[test]
    fn render_ndjson_emits_one_object_per_line() {
        let projects = vec![
            project("1", "Inbox", None),
            project("2", "Errands", Some("1")),
        ];
        let out = render_ndjson(&projects);
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("\"name\":\"Inbox\""));
        assert!(out.contains("\"name\":\"Errands\""));
        assert!(out.contains("\"parent_id\":\"1\""));
        assert!(out.contains("\"parent_id\":null"));
    }

    #[test]
    fn render_ndjson_each_line_parses_as_json() {
        let projects = vec![
            project("1", "Inbox", None),
            project("2", "Errands", Some("1")),
        ];
        let out = render_ndjson(&projects);
        for line in out.lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v.get("id").is_some());
            assert!(v.get("name").is_some());
            assert!(v.get("parent_id").is_some());
        }
    }
}
