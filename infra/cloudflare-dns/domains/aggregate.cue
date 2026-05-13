package domain

// Aggregate the per-domain registry into a single value other packages
// can import. Each `<hostname>.cue` file in this directory adds an
// entry like `domains: "<hostname>": schema.#Domain & { ... }`; CUE
// merges entries across files within the same package, so `domains`
// below is the union of every declared entry.
//
// `#KnownHostnames` is the constraint consumers (e.g. `#Deploy.zone`,
// `#Serving.hostnames`) apply to hostname fields. A hostname matches
// if it equals or is a subdomain of a registered zone — registry
// entries are *zones*, but Worker custom domains and DNS records live
// at arbitrary subdomains beneath them. A typo in either the apex or
// a subdomain fails `cue vet` because no registered zone is a suffix
// of the typo, but `www.kolohelios.com` (subdomain of a registered
// zone) is accepted.
//
// The dot in each zone name is escaped because in a regex `.` matches
// any character; without escaping, `kolohelios.com` would match
// `koloheliosXcom`. The `(^|\.)` prefix anchors the zone at either
// the start of the hostname or a dot boundary, so `.kolohelios.com`
// matches but a hostname like `evilkolohelios.com` doesn't.

import (
	"strings"
	schema "kolohelios.com/tools/shaka/schema/domain"
)

domains: [string]: schema.#Domain

#KnownHostnames: or([
	for k, _ in domains {
		=~"(^|\\.)\(strings.Replace(k, ".", "\\.", -1))$"
	},
])
