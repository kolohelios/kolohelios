package project

// `#D1Database.binding` is required — omitting it is rejected.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target: "cloudflare-worker"
		d1: {
			databaseName: "buzzingo-users"
		}
	}
}
