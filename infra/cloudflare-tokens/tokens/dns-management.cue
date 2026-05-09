package cloudflare_token

// First TF-managed Cloudflare API token, consumed by `infra/cloudflare-dns`
// for the kolohelios.com zone (and future zones). 90-day expiry; rotate
// via `tofu apply -replace=cloudflare_api_token.dns_management` until
// #290 lands automated rotation.

#Token & {
	name:    "dns-management"
	purpose: "Terraform-managed DNS for all zones in the kolohelios account"
	permission_groups: [
		"Account Settings:Read",
		"Zone:Zone:Edit",
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-07T00:00:00Z"
}
