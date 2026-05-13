// CUE module root. Lets package files spread across the tree
// (project schema, domain schema, domain registry, etc.) reference
// each other by absolute import path — e.g.
// `import "kolohelios.com/tools/shaka/schema/domain"` from the
// project schema. Without a module root, CUE can only evaluate files
// that share a single directory, which precludes cross-cutting
// constraints like CUE-enforced hostname-from-registry.
module: "kolohelios.com"
language: version: "v0.15.1"
