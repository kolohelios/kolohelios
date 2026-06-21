package project

// Worker that declares only a D1 database — no custom-domain
// attachment. D1 is account-scoped, so `customDomain`/`zone` aren't
// required; the deploy block's no-attachment branch accepts a D1 alone.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target: "cloudflare-worker"
		d1: {
			binding:      "DB"
			databaseName: "buzzingo-users"
		}
	}
}
