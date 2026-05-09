package cloudflare_token

#Token & {
	name:    "unknown-perm"
	purpose: "rejected because permission group is not in the closed enum"
	permission_groups: [
		"Zone:DNS:Edit",
		"Account:Workers Scripts:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-09T00:00:00Z"
}
