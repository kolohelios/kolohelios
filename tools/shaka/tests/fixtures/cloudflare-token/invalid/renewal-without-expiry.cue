package cloudflare_token

#Token & {
	name: "deploy"
	purpose: "Renewal requires a declared expiry"
	permission_groups: ["Account:Workers Scripts:Edit"]
	scope: {type: "account", account: "kolohelios"}
	renewal: {
		valid_days: 90
		renew_before_days: 14
		secret_ref: "op://vault/item/password"
	}
}
