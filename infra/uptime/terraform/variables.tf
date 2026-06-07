variable "apex_url" {
  description = "Apex URL probed by the HTTP(S) status monitor."
  type        = string
  default     = "https://kolohelios.com"
}

variable "apex_domain" {
  description = "Apex domain the DNS monitor asks the resolver to resolve (request_body for the dns monitor_type)."
  type        = string
  default     = "kolohelios.com"
}

variable "dns_resolver" {
  description = "DNS server the DNS monitor queries (url for the dns monitor_type). A public recursive resolver surfaces delegation/zone failures from outside Cloudflare."
  type        = string
  default     = "1.1.1.1"
}

variable "check_frequency" {
  description = "Check interval in seconds for both monitors. Defaults to 180 (3 min), the Better Stack free-tier default; #196 asked for 5 min but we match the free tier rather than force a stricter-than-allowed value."
  type        = number
  default     = 180
}
