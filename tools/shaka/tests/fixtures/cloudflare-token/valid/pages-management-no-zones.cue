package cloudflare_token

// Pages-management is account-scoped and grants no zone-level
// permissions, so the `zones` field is legitimately omitted.
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
	expires_on: "2026-08-09T00:00:00Z"
}
