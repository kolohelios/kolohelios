package project

// Worker deploy with optional cache rules. `bypassPaths` must begin
// with "/"; non-empty is exercised here, empty list is also valid.
#Project & {
	name: "portfolio-cached"
	kind: "rust-worker"
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "cached.kolohelios.com"
		zone:         "kolohelios.com"
		cache: {
			bypassPaths: ["/api/", "/health"]
		}
	}
}
