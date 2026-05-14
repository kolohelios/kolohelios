package cloudflare_token

// CF Worker-deploy token. Used for three flows:
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
//     `Cache Rules:Edit`, separate from `DNS:Edit`.
//
// Sharing one token for all three flows keeps the credential surface
// tight; if rotation cadences diverge, split later.
//
// 90-day expiry, mirroring `dns-management`; rotate via
// `tofu apply -replace=cloudflare_api_token.deploy` until #290 lands
// automated rotation.

#Token & {
	name:    "deploy"
	purpose: "Worker deploys (TF custom-domain attach + wrangler code uploads + cache rules)"
	permission_groups: [
		"Account:Workers Scripts:Edit",
		"Zone:Cache Rules:Edit",
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-11T00:00:00Z"
}
