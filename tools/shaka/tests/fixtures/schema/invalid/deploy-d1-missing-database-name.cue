package project

// `#D1Database.databaseName` is required — omitting it is rejected.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target: "cloudflare-worker"
		d1: {
			binding: "DB"
		}
	}
}
