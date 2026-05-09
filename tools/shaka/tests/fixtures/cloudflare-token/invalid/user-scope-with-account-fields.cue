package cloudflare_token

#Token & {
	name:    "user-scope-with-account-fields"
	purpose: "rejected because user scope is closed and rejects account fields"
	permission_groups: [
		"User:API Tokens:Edit",
	]
	scope: {
		type:    "user"
		account: "kolohelios"
		zones:   "all"
	}
}
