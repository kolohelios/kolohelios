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
			// Worker runtime secrets pushed at deploy from their `op://`
			// refs in `.env.example` by `push-worker-secrets`, so they're
			// reproducible and survive redeploys rather than being set
			// out-of-band. SESSION_SECRET signs the auth cookie the OAuth
			// callback mints; GITHUB_TOKEN authenticates the cold-tier git
			// commit (the DO PUTs each note to notes-store via the Contents
			// API). Without either, the corresponding path throws.
			secrets: ["SESSION_SECRET", "GITHUB_TOKEN"]
		}
	}
}
