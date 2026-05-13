package domain

// Aggregate the per-domain registry into a single value other packages
// can import. Each `<hostname>.cue` file in this directory adds an
// entry like `domains: "<hostname>": schema.#Domain & { ... }`; CUE
// merges entries across files within the same package, so `domains`
// below is the union of every declared entry. `#KnownHostnames` is
// the closed enum of registered names — consumers (e.g.
// `#Project.serving.hostnames`) constrain hostname fields to this set
// so a typo fails `cue vet` rather than only surfacing at TF apply
// time.

import schema "kolohelios.com/tools/shaka/schema/domain"

domains: [string]: schema.#Domain

#KnownHostnames: or([for k, _ in domains {k}])
