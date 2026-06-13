package project

// Worker carrying the `consumesShaka` knob: an external consumer (no
// kolohelios checkout) that needs a FlakeHub-built `shaka` PATH-prepended
// ahead of the `workflowPackages` shim, which can't resolve a `shaka`
// outside the monorepo. The buzzingo shape.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {
		consumesShaka: true
	}
}
