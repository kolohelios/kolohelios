use serde::{Deserialize, Serialize};

pub const API_BASE: &str = "https://api.todoist.com/rest/v2";

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Network(String),
    Other { status: u16, body: String },
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => {
                write!(f, "token invalid; run `todoist auth login` to refresh")
            }
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Other { status, body } => {
                write!(f, "Todoist API returned {status}: {body}")
            }
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Default, Clone)]
pub struct TaskListQuery {
    pub project_id: Option<String>,
    pub filter: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct CreateTaskBody {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub trait TodoistClient {
    fn list_tasks(
        &self,
        token: &str,
        query: &TaskListQuery,
    ) -> Result<Vec<serde_json::Value>, ApiError>;

    fn list_projects(&self, token: &str) -> Result<Vec<Project>, ApiError>;

    fn create_task(
        &self,
        token: &str,
        body: &CreateTaskBody,
    ) -> Result<serde_json::Value, ApiError>;
}

pub struct RealClient {
    pub base: String,
}

impl Default for RealClient {
    fn default() -> Self {
        Self {
            base: API_BASE.to_string(),
        }
    }
}

impl TodoistClient for RealClient {
    fn list_tasks(
        &self,
        token: &str,
        query: &TaskListQuery,
    ) -> Result<Vec<serde_json::Value>, ApiError> {
        let mut req = ureq::get(&format!("{}/tasks", self.base))
            .set("Authorization", &format!("Bearer {token}"));
        if let Some(pid) = &query.project_id {
            req = req.query("project_id", pid);
        }
        if let Some(filter) = &query.filter {
            req = req.query("filter", filter);
        }
        send_json(req.call())
    }

    fn list_projects(&self, token: &str) -> Result<Vec<Project>, ApiError> {
        let req = ureq::get(&format!("{}/projects", self.base))
            .set("Authorization", &format!("Bearer {token}"));
        send_json(req.call())
    }

    fn create_task(
        &self,
        token: &str,
        body: &CreateTaskBody,
    ) -> Result<serde_json::Value, ApiError> {
        let req = ureq::post(&format!("{}/tasks", self.base))
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json");
        let payload =
            serde_json::to_value(body).map_err(|e| ApiError::Network(format!("encode: {e}")))?;
        send_json(req.send_json(payload))
    }
}

fn send_json<T: for<'de> Deserialize<'de>>(
    result: Result<ureq::Response, ureq::Error>,
) -> Result<T, ApiError> {
    match result {
        Ok(response) => response
            .into_json::<T>()
            .map_err(|e| ApiError::Network(format!("could not parse response: {e}"))),
        Err(ureq::Error::Status(401, _)) => Err(ApiError::Unauthorized),
        Err(ureq::Error::Status(status, response)) => {
            let body = response
                .into_string()
                .unwrap_or_else(|_| "<unreadable>".into());
            Err(ApiError::Other { status, body })
        }
        Err(ureq::Error::Transport(t)) => Err(ApiError::Network(t.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_message_directs_to_auth_login() {
        let msg = ApiError::Unauthorized.to_string();
        assert!(msg.contains("todoist auth login"));
    }

    #[test]
    fn network_message_includes_detail() {
        let msg = ApiError::Network("dns failed".into()).to_string();
        assert!(msg.contains("dns failed"));
    }

    #[test]
    fn other_message_includes_status_and_body() {
        let msg = ApiError::Other {
            status: 500,
            body: "boom".into(),
        }
        .to_string();
        assert!(msg.contains("500"));
        assert!(msg.contains("boom"));
    }

    #[test]
    fn create_task_body_omits_unset_fields() {
        let body = CreateTaskBody {
            content: "buy milk".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"content\":\"buy milk\""));
        assert!(!json.contains("project_id"));
        assert!(!json.contains("due_string"));
        assert!(!json.contains("priority"));
        assert!(!json.contains("labels"));
        assert!(!json.contains("description"));
    }

    #[test]
    fn create_task_body_includes_set_fields() {
        let body = CreateTaskBody {
            content: "x".into(),
            project_id: Some("42".into()),
            due_string: Some("tomorrow".into()),
            priority: Some(3),
            labels: vec!["errand".into()],
            description: Some("longer".into()),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"project_id\":\"42\""));
        assert!(json.contains("\"due_string\":\"tomorrow\""));
        assert!(json.contains("\"priority\":3"));
        assert!(json.contains("\"labels\":[\"errand\"]"));
        assert!(json.contains("\"description\":\"longer\""));
    }
}
