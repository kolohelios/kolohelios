package project

#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["kolohelios.com"]
		},
	]
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
		cache: {
			bypassPaths: ["/api/"]
		}
	}
}
