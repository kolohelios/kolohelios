package project

#Project & {
	name: "notes-sync"
	kind: "rust-worker"
	worker: {}
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["notes-sync.kolohelios.com"]
		},
	]
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "notes-sync.kolohelios.com"
		zone:         "kolohelios.com"
	}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "notes-sync"
		}
	}
}
