package project

#Project & {
	name: "notes-web"
	kind: "rust-worker"
	audit: {
		overrides: [
			{
				rule:          "rust-has-tests"
				severity:      "off"
				justification: "Worker is a 404 stub fallthrough; the HTMX shell and the notes-editor bundle serve all content via wrangler [assets]. The editing/sync logic lives in apps/notes-editor (native-tested) and notes-sync."
			},
		]
	}
	serving: [
		{
			via:       "cloudflare-worker"
			hostnames: ["notes.kolohelios.com"]
		},
	]
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "notes.kolohelios.com"
		zone:         "kolohelios.com"
	}
	ci: {
		deploy: {
			reusableWorkflow:    "./.github/workflows/cf-deploy.yml"
			previewScriptPrefix: "notes-web"
			// The editing surface is the Rust-WASM bundle built from
			// apps/notes-editor; `dist/editor/` is a build artifact (not
			// committed). Build it in the editor's own devshell and stage it
			// into this Worker's `[assets]` dir before worker-build/wrangler,
			// so the deployed shell can import `/editor/notes_editor.js`.
			// Mirrors the manual steps in apps/notes-web/README.md.
			preBuildCommand: "nix develop ../notes-editor --command bash -c 'cd ../notes-editor && just wasm-build' && mkdir -p dist/editor && cp ../notes-editor/dist/* dist/editor/"
			// An editor source change must redeploy notes-web — the bundle it
			// ships is rebuilt by preBuildCommand above.
			extraWatchPaths: ["apps/notes-editor/**"]
		}
	}
}
