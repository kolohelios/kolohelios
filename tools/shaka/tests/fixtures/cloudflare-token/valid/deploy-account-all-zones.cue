package cloudflare_token

#Token & {
	name:    "deploy"
	purpose: "Worker deploys (TF custom-domain attach + wrangler code uploads)"
	permission_groups: [
		"Account:Workers Scripts:Edit",
		"Zone:Cache Rules:Edit",
		"Zone:DNS:Edit",
	]
	scope: {
		type:    "account"
		account: "kolohelios"
		zones:   "all"
	}
	expires_on: "2026-08-09T00:00:00Z"
}
