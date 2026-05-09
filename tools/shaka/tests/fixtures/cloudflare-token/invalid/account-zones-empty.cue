package cloudflare_token

#Token & {
	name:    "empty-zones"
	purpose: "rejected because account scope zones list must be non-empty if not \"all\""
	permission_groups: [
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones: []
	}
	expires_on: "2026-08-09T00:00:00Z"
}
