package project

// Worker with both halves of a deploy block: a custom-domain attachment
// and a D1 database. The two coexist — `d1` is optional in the
// attachment branch.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
		d1: {
			binding:      "DB"
			databaseName: "portfolio-store"
		}
	}
}
