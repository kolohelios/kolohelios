package cloudflare_token

// #Token is the per-token registry entry. Each Cloudflare API token we
// provision has one of these declared as a CUE file under
// `infra/cloudflare-tokens/tokens/`.
//
// The shape is consumed by:
//   - terraform (infra/cloudflare-tokens) provisions tokens via the CF
//     provider's `cloudflare_api_token` resource and writes values to
//     1Password via the `onepassword_item` resource.
//   - `shaka token cloudflare schema-check` validates this shape.
//
// `permission_groups` is a list of human-readable names. TF resolves
// each to a CF permission-group ID at plan time via the
// `cloudflare_api_token_permission_groups` data source. The enum is a
// curated subset — extend as new groups become load-bearing.

#Token: {
	// Resource name in TF; also derives the 1Password item title.
	name: string & =~"^[a-z][a-z0-9-]*$"

	// Human-readable description. Written to the token's CF name field.
	purpose: string & !=""

	// Permission groups granted by this token (at least one).
	permission_groups: [#PermissionGroup, ...#PermissionGroup]

	// Resource scope.
	scope: #AccountScope | #UserScope

	// RFC3339 UTC. Recommended for non-meta tokens; absence means no
	// expiry, which is only acceptable for the bootstrap meta-token.
	expires_on?: string & =~"^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}Z$"

	// Optional client IP CIDR allowlist (IPv4 or IPv6, at least one).
	client_ip_cidrs?: [string, ...string]
}

#AccountScope: {
	type: "account"
	// One account today; literal-constrain. Expand to a string-or-enum
	// when a second account becomes load-bearing.
	account: "kolohelios"
	// "all" = all current and future zones in the account; or an
	// explicit list of zone names. Optional — only meaningful for tokens
	// that grant zone-level permissions; absent for tokens whose
	// permissions are purely account-scoped (e.g. Pages).
	zones?: "all" | [string, ...string]
}

#UserScope: {
	type: "user"
}

#PermissionGroup:
	"Account Settings:Read" |
	"Account Settings:Edit" |
	"Account:Cloudflare Pages:Edit" |
	"Account:Workers Scripts:Edit" |
	"Zone:Cache Settings:Edit" |
	"Zone:Zone:Edit" |
	"Zone:DNS:Edit" |
	"User:API Tokens:Edit"
