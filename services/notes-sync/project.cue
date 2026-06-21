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
			// refs in `.env.example` by `push-worker-secrets`. SESSION_SECRET
			// signs the auth cookie minted by the OAuth callback; without it
			// the callback throws ("SESSION_SECRET not set") on every
			// sign-in. Declared here (not set out-of-band) so it's
			// reproducible and survives redeploys.
			secrets: ["SESSION_SECRET"]
		}
	}
}
