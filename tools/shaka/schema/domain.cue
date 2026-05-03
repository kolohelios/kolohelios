package domain

// #Domain is the per-domain inventory entry. Each domain we own has one of
// these declared as a CUE file under `infra/cloudflare-dns/domains/`.
//
// The shape is consumed by:
//   - terraform (infra/cloudflare-dns) provisions CF zones for entries
//     where `nameservers: "cloudflare"`. TF emits a `domain_expectations`
//     output (per-domain { ns_pair, dnssec_enabled }); that output is the
//     stable contract `shaka domain check` reads at check-time, decoupling
//     the check from TF state file shape.
//   - `shaka domain check` for the static fields TF doesn't see:
//     `disposition` and `expiry_warn_days`. The lock requirement is
//     hardcoded — every domain we own must be `clientTransferProhibited`
//     in WHOIS. DNSSEC drift is detected by comparing DS-at-registry to
//     DNSKEY-from-CF dynamically; no expected DS is declared here, since
//     declaring it would invert source-of-truth (the registry is what
//     resolvers see).
//   - `shaka domain inventory` cross-references the projected Hover paste
//     under `tests/fixtures/domain/hover/snapshot-*.json` against the
//     declared registry, reporting adds/removes.
//
// `nameservers` selects which checks apply:
//   - "cloudflare":    NS check expects the TF-output ns_pair; DNSSEC
//                      validated when `dnssec_enabled: true`.
//   - "linode-legacy": migration-pending; NS/DNSSEC checks skipped, but
//                      expiry and lock checks still run.
//   - "unmanaged":     held but not pointed anywhere actively; only
//                      expiry and lock are enforced.

#Domain: {
	name:              string & =~"^[a-z0-9][a-z0-9-]*(\\.[a-z]{2,})+$"
	disposition:       "portfolio-canonical" | "portfolio-alias" | "personal-alt" | "product-reserve" | "park" | "let-expire"
	expiry_warn_days?: int & >0
} & ({
	nameservers:    "cloudflare"
	dnssec_enabled: bool
} | {
	nameservers: "linode-legacy" | "unmanaged"
})
