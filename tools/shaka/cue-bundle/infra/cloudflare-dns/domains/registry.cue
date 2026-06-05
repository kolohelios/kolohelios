package domain

// Permissive stub of the domain registry, bundled into the packaged
// `shaka` so the project schema's
// `import "kolohelios.com/infra/cloudflare-dns/domains:domain"` resolves
// when shaka runs outside the monorepo (no live registry available).
// `#KnownHostnames: _` accepts any hostname — structural validation
// still runs; the hostname-against-registry constraint is only enforced
// when shaka runs inside a checkout with the real registry (the resolver
// prefers the enclosing `cue.mod` over this bundle). See
// `tools/shaka/src/project/schema_check.rs`.
#KnownHostnames: _
