package project

// `#D1Database.binding` must be non-empty — a blank binding is rejected.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target: "cloudflare-worker"
		d1: {
			binding:      ""
			databaseName: "buzzingo-users"
		}
	}
}
