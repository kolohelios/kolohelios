package cloudflare_token

#Token & {
	name: "deploy"
	purpose: "Renewal window cannot cover the entire lifetime"
	permission_groups: ["Account:Workers Scripts:Edit"]
	scope: {type: "account", account: "kolohelios"}
	expires_on: "2027-02-10T00:00:00Z"
	renewal: {
		valid_days: 90
		renew_before_days: 90
		secret_ref: "op://vault/item/password"
	}
}
