package project

// `simple.period` on a ratelimit binding must be 10 or 60 (Cloudflare's
// only supported windows); 30 is rejected.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-05-01"
		unsafe_bindings: [
			{
				name:         "SUBSCRIBE_RATE_LIMITER"
				type:         "ratelimit"
				namespace_id: "1001"
				simple: {
					limit:  5
					period: 30
				}
			},
		]
	}
}
