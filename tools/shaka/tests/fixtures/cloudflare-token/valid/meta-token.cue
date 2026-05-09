package cloudflare_token

// The bootstrap meta-token is hand-minted (TF can't manage itself
// without it), but its declared shape lives here for documentation and
// schema-check coverage. No `expires_on` — annual hand-roll is the
// rotation strategy per #289.
#Token & {
	name:    "meta-token"
	purpose: "Bootstrap token used by TF to manage all other CF API tokens"
	permission_groups: [
		"User:API Tokens:Edit",
	]
	scope: {
		type: "user"
	}
}
