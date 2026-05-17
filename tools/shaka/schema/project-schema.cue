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

// Publication target for a `#CiBuild` job. Each variant emits a
// different post-`nix build` step in the generated `main.yaml` job:
// `flakehub` runs `DeterminateSystems/flakehub-push`; `artifact` runs
// `actions/upload-artifact`. Exactly one variant must apply.
#PublishFlakehub: {
	kind:       "flakehub"
	name:       string & =~"^[a-z][a-z0-9-]+/[a-z][a-z0-9-]+$"
	visibility: "public" | "private"
	rolling:    bool
}

#PublishArtifact: {
	kind:             "artifact"
	name:             string & !=""
	path:             string & !=""
	retentionDays:    int & >=1 & <=90
	compressionLevel: int & >=0 & <=9
}

// Cross-repo notification fired after a successful publish step.
// Authenticated as the `kolohelios-bot` GitHub App; the auth wiring
// is fixed in the generator, this block only declares the target.
#Dispatch: {
	repo:      string & =~"^[a-z][a-z0-9-]+/[a-z][a-z0-9-]+$"
	eventType: string & !=""
}

// CI build job for a project. The generated `main.yaml` gets a
// `build-<jobId>` job that runs after `preflight`, builds via
// `nix build` (or `nix flake check` when `nixCommand: "check"`), and
// publishes per `publish`. `dorny/paths-filter` gates the job to PRs
// that touch this project's path.
//
// The `<slot>/<name>` filesystem location of the project drives the
// build command's path and the FlakeHub `directory` — those aren't
// duplicated here.
#CiBuild: {
	// Key in the `dorny/paths-filter` map and in `changes.outputs.*`.
	// Usually matches the project name; historically diverged for
	// `infra/devbox` (filterKey "image") so the field stays explicit.
	filterKey: string & =~"^[a-z][a-z0-9-]*$"

	// Job identifier — generated job is `build-<jobId>`. Same drift
	// from project name as `filterKey` (devbox → "image").
	jobId: string & =~"^[a-z][a-z0-9-]*$"

	// Displayed as `Build <displayName>` in the GitHub UI.
	displayName: string & !=""

	// `nix build` for derivations; `nix flake check` for lib-only
	// flakes (e.g. `kolohelios-nix`) where eval is the goal and there
	// is no build output to publish — the FlakeHub push uploads the
	// flake source itself.
	nixCommand: *"build" | "check"

	// Optional attr selector: `nix build ./<slot>/<name>#<attr>`.
	// Used by `infra/devbox` for `#linodeImage`.
	attr?: string & =~"^[a-zA-Z][a-zA-Z0-9_-]*$"

	publish: #PublishFlakehub | #PublishArtifact

	dispatch?: [...#Dispatch]
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
	ci?: {
		build?: #CiBuild
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
		build?: #CiBuild
	}
} | {
	kind: "nix-lib"
	ci?: {
		build?: #CiBuild
	}
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
