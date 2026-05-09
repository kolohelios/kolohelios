variable "cloudflare_account_name" {
  description = "The name of the Cloudflare account that owns these tokens (used to look up the account ID for account-scoped tokens)"
  type        = string
}

variable "onepassword_vault_id" {
  description = "UUID of the 1Password vault that receives provisioned token values. Vault name: \"Kolohelios Monorepo\"."
  type        = string
}
