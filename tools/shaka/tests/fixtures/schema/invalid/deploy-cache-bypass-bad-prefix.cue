package project

// bypassPaths entries must begin with "/" — a bare "api/" prefix
// would silently never match anything, so the schema rejects it.
#Project & {
	name: "portfolio-cached"
	kind: "rust-worker"
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "cached.kolohelios.com"
		zone:         "kolohelios.com"
		cache: {
			bypassPaths: ["api/"]
		}
	}
}
