use askama::Template;
use worker::{event, Context, Env, Request, Response, Result};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate;

#[event(fetch)]
async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let html = IndexTemplate
        .render()
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    Response::from_html(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_template_renders_under_construction_copy() {
        let html = IndexTemplate.render().expect("template renders");
        assert!(html.contains("<title>kolohelios"));
        assert!(html.contains("under construction"));
    }
}
