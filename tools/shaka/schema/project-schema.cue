package project

#Namespace: {
	kind:    "tfstate" | "cache" | "assets"
	name:    string & =~"^[a-z][a-z0-9-]*$"
	purpose: string & !=""
}

#AuditOverride: {
	rule:          string & =~"^[a-z][a-z0-9-]*$"
	severity:      "fail" | "off"
	justification: string & !=""
}

// Deploy intent for an app. The TF that materializes the attachment
// (Worker custom domain + zone data source) is generated from this
// block by `shaka deploy generate-tf` and committed under
// `infra/cloudflare-deploy/terraform/generated/<project>.tf`.
//
// Future targets (`cloudflare-pages`, `fly`, `hetzner`, ...) extend
// this as an additional disjunction branch when they have a consumer.
#Deploy: {
	target:       "cloudflare-worker"
	customDomain: string & =~"^[a-z0-9.-]+$"
	zone:         string & =~"^[a-z0-9.-]+$"
}

#Project: {
	name: string & =~"^[a-z][a-z0-9-]*$"
	objectStorage?: {
		namespaces: [...#Namespace]
	}
	audit?: {
		overrides: [...#AuditOverride]
	}
} & ({
	kind: "rust"
	coverage: {
		line: {
			fail: number & >=0 & <=100
		}
		branch: {
			fail: number & >=0 & <=100
		}
	}
	deploy?: #Deploy
} | {
	kind: "infra"
} | {
	kind: "nix-lib"
})
