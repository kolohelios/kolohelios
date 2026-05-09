package cloudflare_token

#Token & {
	name:    "empty-purpose"
	purpose: ""
	permission_groups: [
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-09T00:00:00Z"
}
