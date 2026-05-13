package cloudflare_token

// CF Worker-deploy token. Used for two flows:
//
//   - `infra/cloudflare-deploy` `tofu apply` that attaches Worker
//     custom domains (`cloudflare_workers_custom_domain` needs
//     `Workers Scripts:Edit` plus zone-level DNS for the routing
//     record CF installs alongside the attach).
//   - `wrangler deploy` from a `rust-worker` app (e.g.
//     `apps/kolohelios-portfolio`) that uploads compiled WASM —
//     also `Workers Scripts:Edit`, account-scoped.
//
// Sharing one token for both flows keeps the credential surface tight;
// if the two grow divergent rotation cadences, split later.
//
// 90-day expiry, mirroring `dns-management`; rotate via
// `tofu apply -replace=cloudflare_api_token.deploy` until #290 lands
// automated rotation.

#Token & {
	name:    "deploy"
	purpose: "Worker deploys (TF custom-domain attach + wrangler code uploads)"
	permission_groups: [
		"Account:Workers Scripts:Edit",
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-11T00:00:00Z"
}
