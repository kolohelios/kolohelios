package cloudflare_token

#Token & {
	name:    "office-only-dns"
	purpose: "DNS management restricted to a single office IP range"
	permission_groups: [
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-09T00:00:00Z"
	client_ip_cidrs: ["198.51.100.0/24", "2001:db8::/32"]
}
