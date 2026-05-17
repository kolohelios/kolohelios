package project

import "kolohelios.com/infra/cloudflare-dns/domains:domain"

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

// Cache rules for the deploy's zone. When set, `shaka deploy
// generate-tf` emits a `cloudflare_ruleset` (phase
// `http_request_cache_settings`) alongside the custom-domain
// attachment. Opinionated defaults bake the rule shape into the
// generator (POSTs always bypass; static-asset extensions and TTLs
// are fixed); knobs land here when a second consumer needs them.
#CacheRules: {
	// Path prefixes (e.g. "/api/") to bypass caching. POSTs are
	// bypassed unconditionally and don't need to be listed.
	bypassPaths: [...string & =~"^/"]
}

// Deploy intent for an app. The TF that materializes the attachment
// (Worker custom domain + zone data source, plus optional cache
// ruleset) is generated from this block by `shaka deploy generate-tf`
// and committed under
// `infra/cloudflare-deploy/terraform/generated/<project>.tf`.
//
// `customDomain` and `zone` are constrained to the registered
// hostnames in `infra/cloudflare-dns/domains/` — a typo fails
// `cue vet` before any TF runs.
//
// Future targets (`cloudflare-pages`, `fly`, `hetzner`, ...) extend
// this as an additional disjunction branch when they have a consumer.
#Deploy: {
	target:       "cloudflare-worker"
	customDomain: domain.#KnownHostnames
	zone:         domain.#KnownHostnames
	cache?:       #CacheRules
}

// Where a project serves content. Decoupled from `#Deploy` (which is
// the implementation detail of how the attachment lands) so future
// projects that register a hostname without an active deploy block
// can still declare intent here.
//
// CUE constrains `hostnames` to the registered set so typos fail
// `cue vet`; cross-project audit rules in `shaka project audit`
// enforce uniqueness (no two projects claim the same hostname) and
// disposition compatibility (the domain's `disposition` matches what
// the `via` here implies).
#Serving: {
	via: "cloudflare-worker" | "external"
	hostnames: [domain.#KnownHostnames, ...domain.#KnownHostnames]
}

#Project: {
	name: string & =~"^[a-z][a-z0-9-]*$"
	objectStorage?: {
		namespaces: [...#Namespace]
	}
	audit?: {
		overrides: [...#AuditOverride]
	}
	serving?: [#Serving, ...#Serving]
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
	// CI/CD workflow configuration. Workflow files under
	// `.github/workflows/` are generated from this block by
	// `shaka ci generate-workflows`; drift is caught in preflight.
	ci?: {
		// Wires this project into a reusable `tofu apply` workflow.
		// Generates `.github/workflows/<name>-apply.yml` that calls
		// `reusable_workflow` with `project_dir = <slot>/<name>`,
		// path-filtered to changes in this project plus the two
		// workflow files involved.
		apply?: {
			reusable_workflow: =~"^\\./\\.github/workflows/[a-z0-9-]+\\.ya?ml$"
		}
	}
} | {
	kind: "nix-lib"
} | {
	// Pandoc-rendered document project. Source `*.md` files are
	// rendered to same-named `*.pdf` (via tectonic) and `*.docx` (via
	// pandoc's native writer). The generated artifacts are committed,
	// and `just validate` re-renders to a temp dir and byte-compares
	// against the committed outputs — drift fails CI. Builds are made
	// byte-reproducible via `SOURCE_DATE_EPOCH=0` and a pinned nix
	// closure for the toolchain.
	kind: "document"
})
