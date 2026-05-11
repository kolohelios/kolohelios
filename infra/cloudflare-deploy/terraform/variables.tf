variable "cloudflare_account_id" {
  description = "Cloudflare account ID. Stable and not secret; visible in the dashboard URL. Passed literally rather than looked up because the deploy-scoped token can't list accounts (matches the pattern in infra/cloudflare-dns; see #311/#313)."
  type        = string
}
