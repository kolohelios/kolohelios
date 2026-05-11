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
} | {
	// Rust crate that compiles to wasm32-unknown-unknown and ships
	// as a Cloudflare Worker. Coverage is optional because cargo-llvm-cov
	// can't measure the wasm-target code paths that actually serve
	// requests; native-only coverage would gate the wrong thing.
	// Deploy lives here (not on `rust`) since `cloudflare-worker` only
	// makes sense for wasm builds.
	kind: "rust-worker"
	coverage?: {
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
