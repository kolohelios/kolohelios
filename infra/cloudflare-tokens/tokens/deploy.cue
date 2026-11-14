package cloudflare_token

// CF Worker-deploy token. Used for four flows:
//
//   - `infra/cloudflare-deploy` `tofu apply` that attaches Worker
//     custom domains (`cloudflare_workers_custom_domain` needs
//     `Workers Scripts:Edit` plus zone-level DNS for the routing
//     record CF installs alongside the attach).
//   - `wrangler deploy` from a `rust-worker` app (e.g.
//     `apps/kolohelios-portfolio`) that uploads compiled WASM —
//     also `Workers Scripts:Edit`, account-scoped.
//   - `infra/cloudflare-deploy` `tofu apply` that materializes
//     per-project cache rules (`cloudflare_ruleset`, phase
//     `http_request_cache_settings`) for the zones the Workers
//     attach to. The rulesets endpoint is gated by
//     `Cache Settings:Edit`, separate from `DNS:Edit`. The grant
//     is broader than "cache rules" — it covers all cache settings
//     on the zone, which is the smallest scope CF exposes.
//   - `infra/cloudflare-deploy` `tofu apply` that creates the
//     portfolio's `cloudflare_web_analytics_site` (#195). The RUM
//     `site_info` endpoints require `Account Settings:Read` for GET/list
//     and `Account Settings:Edit` for create/update/delete. Edit does not
//     satisfy the GET permission: Terraform refresh needs Read explicitly
//     (#1020). Both grants are scoped to the same account.
//
// Sharing one token for all four flows keeps the credential surface
// tight; if rotation cadences diverge, split later.
//
// 90-day expiry, mirroring `dns-management`; `shaka token cloudflare renew`
// advances the declarations before expiry and triggers rotation (#290).

#Token & {
	name:    "deploy"
	purpose: "Worker deploys (TF custom-domain attach + wrangler code uploads + cache rules)"
	permission_groups: [
		"Account:Workers Scripts:Edit",
		"Account Settings:Read",
		"Account Settings:Edit",
		"Zone:Cache Settings:Edit",
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2027-02-10T00:00:00Z"
	renewal: {
		valid_days:        90
		renew_before_days: 14
		secret_ref:        "op://vedq2v6cmtkglnonkenrjneepa/Cloudflare Deploy Token/password"
	}
}
