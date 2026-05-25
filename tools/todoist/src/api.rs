use serde::Deserialize;

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

pub trait TodoistClient {
    fn list_tasks(
        &self,
        token: &str,
        query: &TaskListQuery,
    ) -> Result<Vec<serde_json::Value>, ApiError>;

    fn list_projects(&self, token: &str) -> Result<Vec<Project>, ApiError>;
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
        send_json(req)
    }

    fn list_projects(&self, token: &str) -> Result<Vec<Project>, ApiError> {
        let req = ureq::get(&format!("{}/projects", self.base))
            .set("Authorization", &format!("Bearer {token}"));
        send_json(req)
    }
}

fn send_json<T: for<'de> Deserialize<'de>>(req: ureq::Request) -> Result<T, ApiError> {
    match req.call() {
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
}
