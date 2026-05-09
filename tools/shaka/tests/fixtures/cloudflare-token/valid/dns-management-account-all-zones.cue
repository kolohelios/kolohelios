package cloudflare_token

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
	expires_on: "2026-08-09T00:00:00Z"
}
