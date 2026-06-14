// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

use std::collections::HashMap;

use worker::{
    console_error, event, Context, Env, Fetch, Headers, Method, Request, RequestInit, Response,
    Result,
};

mod subscribe;

use subscribe::{
    kit_payload, validate, Kind, Rejection, FIELD_EMAIL, FIELD_FIRST_NAME, FIELD_HONEYPOT,
    FIELD_KIND, FIELD_MESSAGE,
};

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if req.method() == Method::Post && req.path() == "/api/subscribe" {
        return handle_subscribe(req, env).await;
    }
    Response::error("Not Found", 404)
}

/// Cloudflare rate-limit binding name (see `[[unsafe.bindings]]` in
/// `wrangler.toml`). Keyed per client IP so one source can't flood Kit.
const RATE_LIMIT_BINDING: &str = "SUBSCRIBE_RATE_LIMITER";

/// Parse the submitted form, reject spam/invalid input, and forward a
/// valid submission to Kit. The Kit API key never leaves the worker.
async fn handle_subscribe(mut req: Request, env: Env) -> Result<Response> {
    // Rate-limit by client IP before doing any work. A missing
    // CF-Connecting-IP (only absent outside Cloudflare's edge) collapses
    // to a shared bucket rather than bypassing the limit.
    let ip = req
        .headers()
        .get("CF-Connecting-IP")?
        .unwrap_or_else(|| "unknown".to_string());
    if !env
        .rate_limiter(RATE_LIMIT_BINDING)?
        .limit(ip)
        .await?
        .success
    {
        return Ok(Response::from_html(page(
            "Too many requests",
            "Slow down",
            "You've sent too many submissions. Please wait a minute and try again.",
        ))?
        .with_status(429));
    }

    let form = req.form_data().await?;
    let mut fields = HashMap::new();
    for name in [
        FIELD_KIND,
        FIELD_EMAIL,
        FIELD_FIRST_NAME,
        FIELD_MESSAGE,
        FIELD_HONEYPOT,
    ] {
        if let Some(worker::FormEntry::Field(value)) = form.get(name) {
            fields.insert(name.to_string(), value);
        }
    }

    let submission = match validate(&fields) {
        Ok(submission) => submission,
        // Answer bots with the same success page so the honeypot gives
        // them no signal — but never call Kit.
        Err(Rejection::Honeypot) => return success_response(),
        Err(reason) => {
            return reject(match reason {
                Rejection::InvalidEmail => "Please enter a valid email address.",
                Rejection::MissingMessage => "Please include a message.",
                _ => "That submission could not be processed.",
            })
        }
    };

    let api_key = env.secret("KIT_API_KEY")?.to_string();
    let form_id = match submission.kind {
        Kind::Contact => env.var("KIT_FORM_ID_CONTACT")?.to_string(),
        Kind::Newsletter => env.var("KIT_FORM_ID_NEWSLETTER")?.to_string(),
    };

    let payload = kit_payload(&submission, &api_key);
    let body = serde_json::to_string(&payload)
        .map_err(|e| worker::Error::RustError(format!("serialize kit payload: {e}")))?;

    let headers = Headers::new();
    headers.set("Content-Type", "application/json; charset=utf-8")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(body.into()));

    let url = format!("https://api.convertkit.com/v3/forms/{form_id}/subscribe");
    let kit_req = Request::new_with_init(&url, &init)?;
    let resp = Fetch::Request(kit_req).send().await?;

    if (200..300).contains(&resp.status_code()) {
        success_response()
    } else {
        // Don't leak Kit's response to the client; log it for ourselves.
        console_error!("kit subscribe failed: HTTP {}", resp.status_code());
        Ok(Response::from_html(page(
            "Something went wrong",
            "Something went wrong",
            "We couldn't process your submission just now. Please try again in a moment.",
        ))?
        .with_status(502))
    }
}

fn success_response() -> Result<Response> {
    Response::from_html(page(
        "Thanks",
        "Thanks!",
        "Check your inbox to confirm — you'll need to click the confirmation link.",
    ))
}

fn reject(message: &str) -> Result<Response> {
    Ok(Response::from_html(page("Submission rejected", "Hmm.", message))?.with_status(400))
}

/// A self-contained HTML page for the worker's responses. It links the
/// site's `/style.css` purely for the `:root` design tokens and styles
/// inline with `var(--…)`, so it doesn't depend on Tailwind's content
/// scanner emitting any particular utility class.
fn page(title: &str, heading: &str, message: &str) -> String {
    format!(
        "<!DOCTYPE html>\n\
<html lang=\"en\">\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
<title>{title} — kolohelios</title>\n\
<link rel=\"stylesheet\" href=\"/style.css\">\n\
</head>\n\
<body style=\"min-height:100vh;background:var(--bg);color:var(--fg-strong);font-family:system-ui,sans-serif\">\n\
<main style=\"max-width:48rem;margin:0 auto;padding:3rem 1.5rem\">\n\
<h1 style=\"font-size:1.875rem;font-weight:600;letter-spacing:-0.025em\">{heading}</h1>\n\
<p style=\"color:var(--fg);margin-top:1rem;line-height:1.625\">{message}</p>\n\
<p style=\"margin-top:1.5rem\"><a href=\"/contact\" style=\"color:var(--fg-muted)\">&larr; back to contact</a></p>\n\
</main>\n\
</body>\n\
</html>\n"
    )
}
