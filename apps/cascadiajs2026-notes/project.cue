package project

#Project & {
	name: "cascadiajs2026-notes"
	kind: "rust-worker"
	audit: {
		overrides: [
			{
				rule:          "rust-has-tests"
				severity:      "off"
				justification: "Worker is a stub fallthrough; static assets serve all content via wrangler [assets]. build-site is a single-purpose static generator exercised by the build-check validate step."
			},
		]
	}
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["cascadiajs2026.kolohelios.com"]
		},
	]
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "cascadiajs2026.kolohelios.com"
		zone:         "kolohelios.com"
	}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "cascadiajs2026-notes"
		}
	}
}
