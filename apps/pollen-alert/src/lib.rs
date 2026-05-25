pub mod alert;
pub mod scoring;

use worker::{event, Context, Env, Request, Response, Result, ScheduleContext, ScheduledEvent};

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, _env: Env, _ctx: ScheduleContext) {
    // Stub. Pipeline lands in #409 (scheduled handler) reading from
    // wrangler.toml [vars] and dispatching to the notifier.
}

#[event(fetch)]
async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    Response::error("Not Found", 404)
}
