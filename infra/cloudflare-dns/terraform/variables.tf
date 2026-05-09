variable "cloudflare_account_id" {
  description = "Cloudflare account ID for the zones managed here. Stable and not secret; visible in the dashboard URL. Passed literally rather than looked up because the consumer-token can't list accounts (matches the pattern in infra/cloudflare-tokens; see #311/#313)."
  type        = string
}
