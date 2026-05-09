package cloudflare_token

#Token & {
	name:    "kolohelios-com-dns"
	purpose: "DNS management scoped to kolohelios.com only"
	permission_groups: [
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones: ["kolohelios.com"]
	}
	expires_on: "2026-08-09T00:00:00Z"
}
