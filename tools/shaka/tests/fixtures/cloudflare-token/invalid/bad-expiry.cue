package cloudflare_token

#Token & {
	name:    "bad-expiry"
	purpose: "rejected because expires_on must be RFC3339 UTC"
	permission_groups: [
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-09"
}
