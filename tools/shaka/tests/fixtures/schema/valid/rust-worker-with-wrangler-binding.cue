package project

// Worker with a `ratelimit` [[unsafe.bindings]] entry (the portfolio's
// /api/subscribe rate limiter) alongside assets, vars, and a secret.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-05-01"
		assets: {
			directory: "./dist"
		}
		vars: {
			KIT_FORM_ID_CONTACT: "9525720"
		}
		secrets: ["KIT_API_KEY"]
		unsafe_bindings: [
			{
				name:         "SUBSCRIBE_RATE_LIMITER"
				type:         "ratelimit"
				namespace_id: "1001"
				simple: {
					limit:  5
					period: 60
				}
			},
		]
	}
}
