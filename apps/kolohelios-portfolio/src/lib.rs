use worker::{event, Context, Env, Request, Response, Result};

#[event(fetch)]
async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    Response::error("Not Found", 404)
}
