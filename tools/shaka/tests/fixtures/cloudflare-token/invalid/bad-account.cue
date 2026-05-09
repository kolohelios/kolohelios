package cloudflare_token

#Token & {
	name:    "bad-account"
	purpose: "rejected because account must be the literal kolohelios"
	permission_groups: [
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "some-other-account"
		zones:   "all"
	}
	expires_on: "2026-08-09T00:00:00Z"
}
