package domain

// Same key as `alpha-two.cue` with conflicting values — CUE refuses
// to unify them and the inventory `cue export` step surfaces a
// conflict error. The fixture name is historical; "duplicate"
// hostnames now means "same key, conflicting bodies" rather than
// "two files mention the same name regardless of content."
domains: "alpha.example": {
	disposition:    "personal-alt"
	nameservers:    "cloudflare"
	dnssec_enabled: false
}
