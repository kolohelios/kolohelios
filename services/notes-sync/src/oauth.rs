//! Hand-rolled ATProto OAuth, authentication only. The Worker is the
//! OAuth client: it serves the client-metadata JSON (its public URL is
//! the `client_id`), runs PAR and the token exchange with DPoP, verifies
//! the account DID against the one resolved up front, then discards the
//! ATProto tokens and mints its own signed session cookie.
//!
//! Per-flow state (PKCE verifier, DPoP key, resolved DID/issuer/token
//! endpoint) lives in the `AuthFlow` Durable Object keyed by the
//! `state` value, so nothing sensitive round-trips through the browser.
//! The HTTP/DPoP-nonce glue is wasm-only and deploy-verified; the
//! client-metadata builder is pure and native-tested, and the security
//! checks it leans on (`verify_identity`, `pkce_challenge`, the cookie,
//! the DPoP proof) are tested in their own modules.

use serde_json::{json, Value};

/// The client-metadata document. Its own public URL is the `client_id`.
/// Authentication-only: minimal `atproto` scope, a public client
/// (`token_endpoint_auth_method: none`), DPoP-bound.
pub fn client_metadata(base_url: &str) -> Value {
    let base = base_url.trim_end_matches('/');
    json!({
        "client_id": format!("{base}/client-metadata.json"),
        "client_name": "kolohelios notes",
        "client_uri": base,
        "application_type": "web",
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "redirect_uris": [format!("{base}/oauth/callback")],
        "scope": "atproto",
        "token_endpoint_auth_method": "none",
        "dpop_bound_access_tokens": true,
    })
}

#[cfg(target_arch = "wasm32")]
pub use flow::{handle_callback, handle_login, AuthFlow};

#[cfg(target_arch = "wasm32")]
mod flow {
    use crate::auth::{mint_session, pkce_challenge, verify_identity};
    use crate::dpop::DpopKey;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    // `wasm_bindgen` must be in scope by name: the `#[durable_object]`
    // macro expands to glue that references it (see runtime.rs).
    use worker::{
        console_log, durable_object, wasm_bindgen, Date, DurableObject, Env, Fetch, Headers,
        Method, Request, RequestInit, Response, ResponseBuilder, Result, State,
    };

    /// Session cookie lifetime.
    const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;
    /// Flow state expires if the user doesn't return from the authz server.
    const FLOW_TTL_SECS: i64 = 10 * 60;

    fn now_secs() -> i64 {
        (Date::now().as_millis() / 1000) as i64
    }

    /// A fresh random token (base64url, 32 bytes) for the PKCE verifier,
    /// the `state`, and DPoP `jti`s.
    fn random_token() -> String {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("rng");
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn err(msg: impl std::fmt::Display) -> worker::Error {
        worker::Error::RustError(msg.to_string())
    }

    /// Per-flow state, persisted in the AuthFlow DO between `/oauth/login`
    /// and `/oauth/callback`.
    #[derive(Serialize, Deserialize)]
    struct FlowState {
        verifier: String,
        dpop_key: Vec<u8>,
        did: String,
        issuer: String,
        token_endpoint: String,
    }

    /// One Durable Object per in-flight OAuth flow (`idFromName(state)`).
    /// Strongly consistent so the callback reliably reads what login
    /// wrote; a short alarm cleans it up if the user never returns.
    #[durable_object]
    pub struct AuthFlow {
        state: State,
        #[allow(dead_code)]
        env: Env,
    }

    impl DurableObject for AuthFlow {
        fn new(state: State, env: Env) -> Self {
            Self { state, env }
        }

        async fn fetch(&self, mut req: Request) -> Result<Response> {
            match req.path().as_str() {
                "/put" => {
                    let flow: FlowState = req.json().await?;
                    self.state.storage().put("flow", &flow).await?;
                    self.state
                        .storage()
                        .set_alarm(std::time::Duration::from_secs(FLOW_TTL_SECS as u64))
                        .await?;
                    Response::ok("stored")
                }
                "/take" => {
                    let flow: Option<FlowState> = self.state.storage().get("flow").await?;
                    // One-shot: consume it so a code can't be replayed.
                    let _ = self.state.storage().delete("flow").await;
                    match flow {
                        Some(f) => Response::from_json(&f),
                        None => Response::error("no such flow", 404),
                    }
                }
                _ => Response::error("not found", 404),
            }
        }

        async fn alarm(&self) -> Result<Response> {
            // Expired without a callback — drop the state.
            let _ = self.state.storage().delete("flow").await;
            Response::ok("expired")
        }
    }

    async fn auth_flow_stub(env: &Env, state: &str) -> Result<worker::Stub> {
        env.durable_object("AUTH_FLOW")?
            .id_from_name(state)?
            .get_stub()
    }

    // ---- HTTP helpers --------------------------------------------------

    async fn get_json(url: &str) -> Result<Value> {
        let mut resp = Fetch::Url(url.parse().map_err(err)?).send().await?;
        if resp.status_code() != 200 {
            return Err(err(format!("GET {url} -> {}", resp.status_code())));
        }
        resp.json().await
    }

    /// POST a form body with a DPoP proof, retrying once with the
    /// server-issued nonce (the documented PAR/token first-response is a
    /// `use_dpop_nonce` error carrying a `DPoP-Nonce` header).
    async fn dpop_post_form(url: &str, params: &[(&str, &str)], key: &DpopKey) -> Result<Response> {
        let body = serde_urlencoded::to_string(params).map_err(err)?;
        let mut nonce: Option<String> = None;
        let mut last: Option<Response> = None;
        for _ in 0..2 {
            let proof = key.proof("POST", url, nonce.as_deref(), &random_token(), now_secs());
            let headers = Headers::new();
            headers.set("Content-Type", "application/x-www-form-urlencoded")?;
            headers.set("DPoP", &proof)?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_body(Some(body.clone().into()));
            let req = Request::new_with_init(url, &init)?;
            let resp = Fetch::Request(req).send().await?;
            let code = resp.status_code();
            if (code == 400 || code == 401) && nonce.is_none() {
                if let Ok(Some(n)) = resp.headers().get("DPoP-Nonce") {
                    nonce = Some(n);
                    last = Some(resp);
                    continue;
                }
            }
            return Ok(resp);
        }
        last.ok_or_else(|| err("dpop_post_form produced no response"))
    }

    // ---- Identity & metadata resolution --------------------------------

    /// Resolve a handle (or pass a DID through) to a DID, then to the PDS,
    /// then to the authorization server's metadata.
    async fn resolve(identifier: &str, env: &Env) -> Result<(String, AuthzMeta)> {
        let did = if identifier.starts_with("did:") {
            identifier.to_owned()
        } else {
            let resolver = env
                .var("OAUTH_HANDLE_RESOLVER")
                .map(|v| v.to_string())
                .unwrap_or_else(|_| "https://bsky.social".to_owned());
            let url = format!(
                "{}/xrpc/com.atproto.identity.resolveHandle?handle={}",
                resolver.trim_end_matches('/'),
                identifier
            );
            get_json(&url).await?["did"]
                .as_str()
                .ok_or_else(|| err("resolveHandle: no did"))?
                .to_owned()
        };

        let pds = did_to_pds(&did).await?;
        let meta = pds_to_authz(&pds).await?;
        Ok((did, meta))
    }

    async fn did_to_pds(did: &str) -> Result<String> {
        // did:plc via plc.directory; did:web by fetching its document.
        let doc_url = if let Some(rest) = did.strip_prefix("did:web:") {
            format!("https://{rest}/.well-known/did.json")
        } else {
            format!("https://plc.directory/{did}")
        };
        let doc = get_json(&doc_url).await?;
        doc["service"]
            .as_array()
            .and_then(|services| {
                services
                    .iter()
                    .find(|s| s["type"].as_str() == Some("AtprotoPersonalDataServer"))
            })
            .and_then(|s| s["serviceEndpoint"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| err("DID doc has no PDS service endpoint"))
    }

    struct AuthzMeta {
        issuer: String,
        par_endpoint: String,
        authorization_endpoint: String,
        token_endpoint: String,
    }

    async fn pds_to_authz(pds: &str) -> Result<AuthzMeta> {
        let prot = get_json(&format!(
            "{}/.well-known/oauth-protected-resource",
            pds.trim_end_matches('/')
        ))
        .await?;
        let authz = prot["authorization_servers"][0]
            .as_str()
            .ok_or_else(|| err("no authorization_servers"))?
            .to_owned();
        let meta = get_json(&format!(
            "{}/.well-known/oauth-authorization-server",
            authz.trim_end_matches('/')
        ))
        .await?;
        let s = |k: &str| {
            meta[k]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| err(format!("authz metadata missing {k}")))
        };
        Ok(AuthzMeta {
            issuer: authz,
            par_endpoint: s("pushed_authorization_request_endpoint")?,
            authorization_endpoint: s("authorization_endpoint")?,
            token_endpoint: s("token_endpoint")?,
        })
    }

    fn base_url(env: &Env) -> Result<String> {
        Ok(env
            .var("OAUTH_BASE_URL")
            .map_err(|_| err("OAUTH_BASE_URL not set"))?
            .to_string()
            .trim_end_matches('/')
            .to_owned())
    }

    // ---- Endpoints -----------------------------------------------------

    /// `GET /oauth/login?handle=…` — resolve, PAR, persist flow state, and
    /// redirect the user to the authorization server.
    pub async fn handle_login(req: Request, env: Env) -> Result<Response> {
        let url = req.url()?;
        let handle = url
            .query_pairs()
            .find(|(k, _)| k == "handle")
            .map(|(_, v)| v.into_owned())
            .ok_or_else(|| err("missing ?handle"))?;

        let base = base_url(&env)?;
        let client_id = format!("{base}/client-metadata.json");
        let redirect_uri = format!("{base}/oauth/callback");

        let (did, meta) = resolve(&handle, &env).await?;

        let verifier = random_token();
        let challenge = pkce_challenge(&verifier);
        let state = random_token();
        let key = DpopKey::generate();

        // Pushed Authorization Request.
        let par_params = [
            ("client_id", client_id.as_str()),
            ("response_type", "code"),
            ("scope", "atproto"),
            ("redirect_uri", redirect_uri.as_str()),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("login_hint", handle.as_str()),
        ];
        let mut par_resp = dpop_post_form(&meta.par_endpoint, &par_params, &key).await?;
        if par_resp.status_code() / 100 != 2 {
            return Err(err(format!("PAR -> {}", par_resp.status_code())));
        }
        let request_uri = par_resp.json::<Value>().await?["request_uri"]
            .as_str()
            .ok_or_else(|| err("PAR: no request_uri"))?
            .to_owned();

        // Persist the flow state keyed by `state`.
        let flow = FlowState {
            verifier,
            dpop_key: key.to_bytes(),
            did,
            issuer: meta.issuer.clone(),
            token_endpoint: meta.token_endpoint,
        };
        let stub = auth_flow_stub(&env, &state).await?;
        let put = {
            let headers = Headers::new();
            headers.set("Content-Type", "application/json")?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_body(Some(serde_json::to_string(&flow).map_err(err)?.into()));
            Request::new_with_init("https://auth-flow/put", &init)?
        };
        let _ = stub.fetch_with_request(put).await?;

        // Redirect to the authorization endpoint (PAR-style: client_id +
        // request_uri).
        let location = format!(
            "{}?client_id={}&request_uri={}",
            meta.authorization_endpoint,
            urlencode(&client_id),
            urlencode(&request_uri),
        );
        Response::redirect(location.parse().map_err(err)?)
    }

    /// `GET /oauth/callback?code=…&state=…&iss=…` — exchange the code,
    /// verify the identity, mint the session cookie, discard the tokens.
    pub async fn handle_callback(req: Request, env: Env) -> Result<Response> {
        let url = req.url()?;
        let q = |name: &str| {
            url.query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
        };
        let code = q("code").ok_or_else(|| err("missing ?code"))?;
        let state = q("state").ok_or_else(|| err("missing ?state"))?;
        let iss = q("iss").ok_or_else(|| err("missing ?iss"))?;

        // Take the one-shot flow state.
        let stub = auth_flow_stub(&env, &state).await?;
        let take = Request::new("https://auth-flow/take", Method::Get)?;
        let mut take_resp = stub.fetch_with_request(take).await?;
        if take_resp.status_code() != 200 {
            return Err(err("unknown or expired flow state"));
        }
        let flow: FlowState = take_resp.json().await?;
        let key = DpopKey::from_bytes(&flow.dpop_key).ok_or_else(|| err("bad dpop key"))?;

        let base = base_url(&env)?;
        let client_id = format!("{base}/client-metadata.json");
        let redirect_uri = format!("{base}/oauth/callback");

        // Token exchange.
        let token_params = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", flow.verifier.as_str()),
        ];
        let mut token_resp = dpop_post_form(&flow.token_endpoint, &token_params, &key).await?;
        if token_resp.status_code() / 100 != 2 {
            return Err(err(format!("token -> {}", token_resp.status_code())));
        }
        let body = token_resp.json::<Value>().await?;
        let sub = body["sub"].as_str().ok_or_else(|| err("token: no sub"))?;

        // The mandatory check: sub is the DID we resolved, iss is the
        // authorization server we used.
        verify_identity(sub, &iss, &flow.did, &flow.issuer).map_err(err)?;

        // Authn-only: the ATProto tokens are discarded here (we never store
        // `body["access_token"]`/`refresh_token`). Mint our own cookie.
        let cookie = mint_session(
            &flow.did,
            now_secs() + SESSION_TTL_SECS,
            session_secret(&env)?.as_bytes(),
        );
        console_log!("authenticated {} via {}", flow.did, flow.issuer);

        let headers = Headers::new();
        headers.set("Location", "/")?;
        headers.set(
            "Set-Cookie",
            &format!("session={cookie}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={SESSION_TTL_SECS}"),
        )?;
        Ok(ResponseBuilder::new()
            .with_status(302)
            .with_headers(headers)
            .empty())
    }

    fn session_secret(env: &Env) -> Result<String> {
        Ok(env
            .secret("SESSION_SECRET")
            .map_err(|_| err("SESSION_SECRET not set"))?
            .to_string())
    }

    /// Minimal percent-encoding for the two query values we put in the
    /// authorize redirect (the rest of the flow uses form bodies).
    fn urlencode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_metadata_is_authn_only_and_self_describing() {
        let m = client_metadata("https://notes.example.com/");
        assert_eq!(
            m["client_id"],
            "https://notes.example.com/client-metadata.json"
        );
        assert_eq!(m["scope"], "atproto");
        assert_eq!(m["token_endpoint_auth_method"], "none");
        assert_eq!(m["dpop_bound_access_tokens"], true);
        assert_eq!(m["grant_types"][0], "authorization_code");
        assert_eq!(
            m["redirect_uris"][0],
            "https://notes.example.com/oauth/callback"
        );
    }

    #[test]
    fn client_metadata_trims_a_trailing_slash() {
        let a = client_metadata("https://x.example");
        let b = client_metadata("https://x.example/");
        assert_eq!(a["client_id"], b["client_id"]);
    }
}
