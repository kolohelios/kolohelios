package domain

import schema "kolohelios.com/tools/shaka/schema/domain"

domains: "kolohelios.com": schema.#Domain & {
	name:           "kolohelios.com"
	disposition:    "portfolio-canonical"
	nameservers:    "cloudflare"
	dnssec_enabled: false
	ns_pair: ["kayden.ns.cloudflare.com.", "nora.ns.cloudflare.com."]
}
