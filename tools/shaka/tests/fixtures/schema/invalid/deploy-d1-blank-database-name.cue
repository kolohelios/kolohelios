package project

// `#D1Database.databaseName` must be non-empty — a blank name is
// rejected.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target: "cloudflare-worker"
		d1: {
			binding:      "DB"
			databaseName: ""
		}
	}
}
