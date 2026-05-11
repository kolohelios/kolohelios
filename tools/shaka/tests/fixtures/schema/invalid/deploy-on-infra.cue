package project

// infra projects cannot declare a deploy block; only rust apps deploy.
#Project & {
	name: "cloudflare-deploy"
	kind: "infra"
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
	}
}
