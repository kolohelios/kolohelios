package domain

import schema "kolohelios.com/tools/shaka/schema/domain"

domains: "kolohelios.com": schema.#Domain & {
	name:           "kolohelios.com"
	disposition:    "portfolio-canonical"
	nameservers:    "cloudflare"
	dnssec_enabled: false
}
