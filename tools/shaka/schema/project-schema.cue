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

// Waive a generator step for this project. shaka can't yet produce a
// workspace-aware justfile for a rust-worker Cargo workspace (#922), so such a
// project hand-maintains its justfile and would otherwise fail
// `generate-justfiles --check`. Listing the step here excludes the project from
// BOTH the write pass and the `--check` drift pass. `reason` (non-empty,
// mirroring #AuditOverride.justification) is mandatory so the waiver is
// auditable in git history and self-documents the interim. `step` is an enum so
// future generator artifacts extend it without a schema migration.
#GenerateSkip: {
	step:   "justfile"
	reason: string & !=""
}

#GenerateConfig: {
	skip?: [...#GenerateSkip]
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

	// When true, `shaka deploy generate-tf` emits a
	// `cloudflare_web_analytics_site` (manual install — `auto_install`
	// off) for the custom domain alongside the attachment. The issued
	// `site_token` is read from TF state after apply (`tofu state show
	// 'module.generated.cloudflare_web_analytics_site.<project>'`) and
	// baked into the site's HTML beacon — it's a public, non-secret
	// identifier. Workers Static Assets responses can't be auto-injected,
	// so the beacon is hand-placed in the page shell.
	webAnalytics?: bool
}

// A Cloudflare `[[unsafe.bindings]]` entry — the escape hatch for
// bindings wrangler lacks first-class TOML for. Discriminated on `type`
// so KV/R2/queue variants can extend this disjunction later; only the
// `ratelimit` shape the portfolio's `/api/subscribe` uses is modelled
// now. `namespace_id` is an arbitrary unique id (string); `period` is
// the rate-limit window, which Cloudflare restricts to 10 or 60 seconds.
#RateLimitBinding: {
	name:         string & !=""
	type:         "ratelimit"
	namespace_id: string & !=""
	simple: {
		limit:  int & >0
		period: 10 | 60
	}
}

// Wrangler runtime/build config that drives `wrangler.toml` for a
// `rust-worker` project. `shaka deploy generate-wrangler` (#113) emits
// `wrangler.toml` from this block and drift-checks it in preflight, so
// the hand-maintained TOML in `apps/*/wrangler.toml` becomes generated.
// Distinct from `#Deploy` (DNS + cache rules, emitted as Terraform) and
// from the `worker:` flake-shape block (devShell knobs for
// `generate-flakes`): this block is everything wrangler itself needs to
// build and run the worker. The project's top-level `name` is the source
// of truth for the worker name — not duplicated here.
//
// Covers the two real shapes in the repo: pollen-alert (`build` + `cron`
// + `vars`) and the portfolio (`assets`, no triggers/vars). Every field
// past `main`/`compatibility_date` is optional so both validate.
#Wrangler: {
	// Entry point wrangler uploads — the `worker-build` shim, e.g.
	// `build/worker/shim.mjs`.
	main: string & !=""

	// Cloudflare runtime compatibility date, `YYYY-MM-DD`.
	compatibility_date: string & =~"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"

	// Custom build step (`[build].command`). Omit when the artifact is
	// produced upstream of `wrangler deploy` (the portfolio builds in a
	// dedicated CI/devshell step instead).
	build?: {
		command: string & !=""
	}

	// Workers Static Assets (`[assets].directory`) — CF serves these
	// files at the edge, unmatched paths fall through to `main`.
	assets?: {
		directory: string & !=""
	}

	// Scheduled triggers (`[triggers].crons`): standard 5-field UTC cron
	// expressions.
	cron?: [...string & =~"^(\\S+\\s+){4}\\S+$"]

	// Non-secret runtime config (`[vars]`).
	vars?: {[string]: string}

	// Secret names declared for the worker (set out-of-band via
	// `wrangler secret put`); values never live in the repo. Names only,
	// so the generator can surface them as a comment.
	secrets?: [...string & =~"^[A-Z][A-Z0-9_]*$"]

	// Cloudflare `[[unsafe.bindings]]` entries. Modelled because the
	// portfolio's rate limiter (`SUBSCRIBE_RATE_LIMITER`) can't otherwise
	// round-trip through the generator; dropping it would break
	// `/api/subscribe` rate limiting on deploy.
	unsafe_bindings?: [...#RateLimitBinding]
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

	// After `flakehub-push` registers this revision, also run
	// `nix build https://flakehub.com/f/<name>/*.tar.gz`. The closure
	// produced by that second build has a different `.drv` than the
	// `nix build <local-path>` we already did (source-store-hash
	// differs between path and tarball inputs), so it lands a *new*
	// entry in FlakeHub Cache — the one downstream consumers will
	// look up when they evaluate this flake. Set to `true` for
	// flakes consumed by other repos as `nix build`-able derivations;
	// leave `false` for libs (where there's no built output) or
	// flakes whose only consumer is this repo itself.
	populateConsumerCache: *false | bool
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

// CI deploy job for a Cloudflare Worker project. The generator emits
// `.github/workflows/<name>-deploy.yml` wrapping the shared
// `cf-deploy.yml` reusable workflow: verify (PR), preview (PR), comment
// (PR), and deploy (push:main). The `cleanup` job that runs on
// pull_request:closed stays in a hand-authored sibling
// `<name>-cleanup.yml` — its trigger and concern don't overlap with
// the rest of the deploy lifecycle.
//
// The `<slot>/<name>` filesystem location of the project drives the
// `project_dir` input to the reusable workflow; not duplicated here.
#CiDeploy: {
	// Reusable workflow this project's deploy delegates to. Must match
	// the cf-deploy.yml form for the generated verify/preview/deploy
	// jobs to make sense. Two accepted forms:
	//   - local in-repo: `./.github/workflows/<name>.yml`
	//   - cross-repo:    `<owner>/<repo>/.github/workflows/<name>.yml@<ref>`
	// The cross-repo form lets an external consumer (e.g. the private
	// buzzingo repo) reuse this repo's `cf-deploy.yml` instead of
	// hand-authoring its own deploy machinery.
	reusableWorkflow: =~"^\\./\\.github/workflows/[a-z0-9-]+\\.ya?ml$" |
		=~"^[\\w.-]+/[\\w.-]+/\\.github/workflows/[a-z0-9-]+\\.ya?ml@[\\w./-]+$"

	// PR preview Worker name template: the emitter generates
	// `<previewScriptPrefix>-pr-${{ github.event.pull_request.number }}`
	// as the `script_name_override` for the preview job and as the
	// `--name` flag for the sibling cleanup workflow.
	previewScriptPrefix: string & =~"^[a-z][a-z0-9-]*$"

	// Optional wrangler environment for the production `deploy` job.
	// When set, the generated `deploy` job passes `environment` to
	// `cf-deploy.yml`, which appends `--env <environment>` so wrangler
	// selects a named `[env.<name>]` section of `wrangler.toml`. The
	// `verify`/`preview` jobs never set it: a prod-only `custom_domain`
	// route lives under `[env.production]` while route-free previews
	// stay on `*.workers.dev`. Omit to deploy with the top-level config.
	environment?: string & =~"^[a-z][a-z0-9_-]*$"

	// The Cargo-workspace member crate (a subdir of `project_dir`) that
	// `cf-deploy.yml` runs `worker-build` in, threaded to its `worker_build_dir`
	// input in all three (verify/preview/deploy) `with:` blocks. A workspace
	// Worker's virtual root manifest has no `[package]`, so worker-build must
	// run in the member crate holding the cdylib (e.g. `crates/<name>-server`).
	// Omit for a single-crate Worker (worker-build runs in `project_dir`).
	workerBuildDir?: string & =~"^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$"

	// Worker runtime secret names to push at deploy via
	// `shaka ci push-worker-secrets` (the reusable `cf-deploy.yml` runs
	// it). Names only — values arrive from the resolved `op://`
	// environment at deploy, never the repo. Unlike `wrangler.secrets`,
	// this lives outside `#Wrangler`, so a project with a hand-authored
	// `wrangler.toml` (Durable Objects, migrations, `[env.production]` —
	// none modelled by `#Wrangler`) can declare push-able secrets without
	// enrolling in `generate-wrangler` drift. `push-worker-secrets`
	// pushes the union of these and `wrangler.secrets`.
	secrets?: [...string & =~"^[A-Z][A-Z0-9_]*$"]
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
	generate?: #GenerateConfig
	serving?: [#Serving, ...#Serving]
} & ({
	// Rust binary crate that ships a CLI. `cli:` carries the
	// flake-shape knobs consumed by `shaka project generate-flakes`
	// (binary name, runtime PATH deps, completions, devShell
	// extras). Every existing rust crate in the repo is a CLI; if a
	// non-CLI rust crate ever shows up, branch a new kind.
	kind: "rust-cli"
	coverage: {
		line: {
			fail: number & >=0 & <=100
		}
	}
	ci?: {
		build?: #CiBuild
	}
	cli: {
		// pname for `packages.default` and the file installed to
		// `bin/`. Kept explicit (not derived from `name`) because
		// nothing in the schema enforces they match — a future
		// project might want to ship a binary whose name differs
		// from its project name.
		binaryName: string & =~"^[a-z][a-z0-9-]*$"

		// Top-level flake `description`. Defaults to the project
		// name; override when a longer phrase reads better.
		description?: string & !=""

		// Emit `packages.default` (and matching `apps.default`).
		// Set false for CLIs that are built only via `cargo` and
		// never via `nix build` (e.g. `tools/todoist` today).
		package: *true | false

		// Nixpkgs attrs baked onto the binary's PATH via
		// `wrapProgram --prefix PATH`. Non-empty implies
		// `makeWrapper` in nativeBuildInputs and a postFixup block.
		runtimePathDeps?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]

		// Nixpkgs attrs added to `nativeCheckInputs` so the build
		// sandbox's check phase (and `nix flake check`) can run
		// tests that shell out.
		checkInputs?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]

		// Emit the `installShellCompletion` postInstall plumbing
		// for `<binaryName> completions <shell>`. Off by default
		// (most fresh CLIs don't have the subcommand wired yet);
		// opt in once `clap_complete` is in place.
		shellCompletions: *false | true

		// Extra nixpkgs attrs dropped into the devShell, on top of
		// the rust toolchain and `workflowPackages`.
		extraDevShellPackages?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]

		// Bundle a CUE module closure into the package so schema-
		// dependent subcommands work from a nix-built binary run
		// outside the monorepo (the `cue.mod` + schema + a permissive
		// stub registry aren't in `$PATH` or the build src otherwise).
		// Each `include` copies a directory from the flake src into
		// `$out/share/shaka/cue/<to>` preserving the module-relative
		// layout, and the binary is wrapped with `SHAKA_CUE_MODULE_DIR`
		// pointing there. Only shaka needs this (it ships the project
		// schema it validates against); other CLIs omit the block.
		cueModule?: {
			include: [...{
				from: string & !=""
				to:   string & !=""
			}]
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
	}
	deploy?:   #Deploy
	wrangler?: #Wrangler
	ci?: {
		deploy?: #CiDeploy
	}
	// Flake-shape knobs consumed by `shaka project generate-flakes`.
	// The worker template is fixed (rust-overlay toolchain with the
	// `wasm32-unknown-unknown` target, `wrangler`, `pkg-config` +
	// `openssl` for `worker-build`'s native `cargo install`, the
	// `~/.cargo/bin` PATH `shellHook`); this block carries only the
	// per-project variation. Required so every rust-worker has a
	// generated flake — no hand-authored holdouts.
	worker: {
		// Top-level flake `description`. Defaults to the project
		// name; override when a longer phrase reads better.
		description?: string & !=""

		// Extra nixpkgs attrs dropped into the devShell, on top of
		// the worker toolchain, `wrangler`, `pkg-config`, `openssl`,
		// and `workflowPackages`. `kolohelios-portfolio` lists
		// `tailwindcss` + `cue` for its build path; `pollen-alert`
		// needs none.
		extraDevShellPackages?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]

		// Extra Rust target triples appended to the toolchain's
		// `targets`, on top of the fixed `wasm32-unknown-unknown` the
		// Worker is built for. rust-overlay supplies each target's std
		// components, so this is the only knob a `rust-worker` needs to
		// cross-compile the same crate elsewhere (for example a `uniffi`
		// FFI crate to `aarch64-apple-ios` / `aarch64-linux-android`).
		// Platform cross-linkers/SDKs are out of scope here — the
		// Android NDK and friends go through `extraDevShellPackages`.
		extraRustTargets?: [...string & =~"^[a-z0-9_]+(-[a-z0-9_]+)+$"]

		// Pull a FlakeHub-built `shaka` into the devShell and
		// PATH-prepend it ahead of the `workflowPackages` shim. Only
		// external consumers (repos with no kolohelios checkout) need
		// this: the shim resolves `shaka` by walking up to
		// `tools/shaka/bin/shaka`, which doesn't exist outside the
		// monorepo, so it dies at runtime (`shaka ci mask-and-run`,
		// any local `shaka <cmd>`). When true, `generate-flakes` adds
		// the `shaka` FlakeHub input and a shellHook that puts the real
		// binary on PATH first. Leave default-false for in-repo workers,
		// where the shim finds the monorepo binary.
		consumesShaka: *false | bool
	}
} | {
	// Rust library crate shared across consumers — compiled natively
	// for `cargo test` and to wasm32-unknown-unknown by the Worker and
	// the WASM editor. No binary and no Cloudflare deploy; the wasm
	// target is part of the contract, so the generated `validate`
	// includes a `wasm-check`. `packages/notes-protocol` is the first.
	//
	// Coverage is required (like rust-cli): a shared lib is pure,
	// natively testable logic, so cargo-llvm-cov measures the real
	// surface — unlike rust-worker, whose wasm-only request paths it
	// can't see.
	kind: "rust-lib"
	coverage: {
		line: {
			fail: number & >=0 & <=100
		}
	}
	// Optional FlakeHub publication for a lib consumed by other repos.
	// `notes-protocol` is consumed in-repo via a path dep and omits it.
	ci?: {
		build?: #CiBuild
	}
	rustLib?: {
		// Top-level flake `description`. Defaults to the project name.
		description?: string & !=""

		// Extra nixpkgs attrs dropped into the devShell, on top of the
		// rust toolchain and `workflowPackages`.
		extraDevShellPackages?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]
	}
} | {
	// Browser-wasm application: a Rust crate compiled to
	// wasm32-unknown-unknown for the *browser* (web-sys / wasm-bindgen),
	// not workerd. Built into a static bundle that a rust-worker serves
	// via `[assets]`. `apps/notes-editor` is the first.
	//
	// Coverage is optional (like rust-worker): the DOM and socket paths
	// run only in the browser, so cargo-llvm-cov can't measure them; any
	// pure logic that is natively testable can still opt in.
	kind: "wasm-app"
	coverage?: {
		line: {
			fail: number & >=0 & <=100
		}
	}
	wasmApp?: {
		// Top-level flake `description`. Defaults to the project name.
		description?: string & !=""

		// Extra nixpkgs attrs dropped into the devShell, on top of the
		// rust toolchain, wasm-bindgen-cli, binaryen, and
		// `workflowPackages`.
		extraDevShellPackages?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]
	}
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
	infra?: {
		// Top-level flake `description`. Defaults to project name.
		description?: string & !=""

		// Extra flake inputs beyond the standard kolohelios-nix +
		// nixpkgs. Each key is the input name; value is the URL
		// plus optional follows declarations. Required for projects
		// that consume sibling flakes (devbox → home-env, home →
		// home-manager / nix-darwin / claude-hooks).
		extraInputs?: [Name=string & =~"^[a-z][a-z0-9-]*$"]: {
			url: string & !=""
			// Map of `inputs.<key>.follows = <value>`. The
			// generator emits one `inputs.<src>.follows` line per
			// entry inside the input's block.
			follows?: [Src=string & =~"^[a-z][a-z0-9-]*$"]: string & !=""
		}

		// Nixpkgs attrs added to the devShell's `packages` list, on
		// top of `(workflowPackages pkgs)`. Cloudflare-style
		// Terraform projects typically list `opentofu` and
		// `_1password-cli`; devbox uses `opentofu` and `linode-cli`;
		// home uses none.
		devShellPackages?: [...string & =~"^[a-zA-Z_][a-zA-Z0-9_-]*$"]

		// Raw nix snippet appended inside the standard `let ..` block,
		// before `in { ... }`. Used by projects whose extra outputs
		// share computed values that would otherwise repeat
		// (devbox's `devboxConfig`, `imageConfig`). Optional;
		// projects that don't need shared bindings leave this empty.
		letExtra?: string

		// Raw nix snippet appended inside `outputs.{...}: let .. in
		// { HERE }`, after the standard `devShells` block but before
		// `formatter`. Escape sandbox for project-specific outputs —
		// devbox's `nixosConfigurations` / `packages.linodeImage` /
		// `checks.devbox-eval`, home's `darwinConfigurations` /
		// `nixosModules`. Loses CUE syntax-highlighting and
		// structural validation; the trade-off is keeping every
		// project inside the generator's drift-checked envelope
		// instead of carving exceptions.
		extra?: string
	}
} | {
	kind: "nix-lib"
	ci?: {
		build?: #CiBuild
	}
	nixLib?: {
		// Top-level flake `description`. Defaults to project name.
		description?: string & !=""

		// Raw nix snippet for the entire `let .. in { ... }` body
		// of `outputs`. nix-lib *defines* what other flakes import,
		// so its shape isn't a fixed template — the let-bindings,
		// the `lib = { inherit ... }` export, formatter, devShells
		// are all project-specific. The template provides only the
		// flake header, description, nixpkgs input, and outputs
		// signature; everything else lives here. Same
		// escape-sandbox trade-off as `infra.extra` but covering a
		// wider surface.
		extra?: string
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
	document?: {
		// Top-level flake `description`. Defaults to the project
		// name; override when a longer phrase reads better.
		// pandoc + tectonic are baked into the generator's
		// document template (matching the justfile DOCUMENT_TEMPLATE
		// hardcode of `--pdf-engine=tectonic`); if a future document
		// project wants different renderers, this block grows a
		// `packages` knob.
		description?: string & !=""
	}
} | {
	// Native mobile app shell — a SwiftUI/iOS or Jetpack Compose/Android
	// app that links the bindings and libraries a `uniffi` FFI crate
	// produces. One kind covers both platforms because shaka's behavior is
	// identical for each: it never builds the app. iOS needs macOS + Xcode
	// (`xcodebuild`, an XCFramework, a Simulator); Android needs the
	// Android SDK + Gradle (+ NDK for the `.so`s) — none of which the Linux
	// `shaka preflight` lane carries.
	//
	// So `native-app` is a *host-built* kind: the `project.cue` validates
	// (keeping the "every project declares a kind" invariant), but shaka
	// generates no `flake.nix` and no `justfile` for it, and `preflight`
	// skips it in the per-project `just validate` loop rather than failing
	// on a build it can't run. Repo-level checks (whitespace, typos, vale,
	// taplo) still scan its files. The developer assembles the
	// XCFramework/AAR from the FFI crate (`just ffi-ios` / `ffi-android`
	// in the consuming repo), links it, and builds in Xcode/Gradle on a
	// host with the platform toolchain. shaka holds no opinion on project
	// representation (`.xcodeproj` vs SPM vs XcodeGen/Tuist; Gradle
	// layout) — that's the consumer's call until a second consumer wants a
	// shared one.
	kind: "native-app"
	nativeApp: {
		// Target platform. Selects the host toolchain the developer
		// builds with (Xcode for `ios`, Gradle for `android`); shaka
		// treats the two identically otherwise.
		platform: "ios" | "android"

		// The `uniffi` FFI crate (a sibling project elsewhere in the
		// repo, or in a repo this one consumes) whose generated
		// bindings/libraries this app links. Recorded so the app's
		// provenance is declared even though shaka doesn't wire the build.
		ffiCrate: string & =~"^[a-z][a-z0-9-]*$"

		// Reverse-DNS application identifier — the iOS bundle identifier
		// or the Android `applicationId` (same grammar). Lowercase
		// dot-separated segments, e.g. `com.example.app`.
		appId: string & =~"^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+$"
	}
})
