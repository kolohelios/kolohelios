package cloudflare_token

#Token & {
	name:              "empty-perms"
	purpose:           "rejected because permission_groups must be non-empty"
	permission_groups: []
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-09T00:00:00Z"
}
