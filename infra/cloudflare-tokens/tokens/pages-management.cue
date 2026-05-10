package cloudflare_token

// CF Pages-management token consumed by `infra/cloudflare-portfolio` for
// both the `tofu apply` (creating the Pages project) and the `wrangler
// pages deploy` step that uploads `site/`. Account-scoped only — Pages
// has no zone-level resources, so `zones` is omitted.
//
// 90-day expiry; rotate via
// `tofu apply -replace=cloudflare_api_token.pages_management` until #290
// lands automated rotation.

#Token & {
	name:    "pages-management"
	purpose: "Terraform-managed Cloudflare Pages projects + wrangler deploys"
	permission_groups: [
		"Account:Cloudflare Pages:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
	}
	expires_on: "2026-08-07T00:00:00Z"
}
