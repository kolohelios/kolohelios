variable "linode_token" {
  description = "Linode API personal access token"
  type        = string
  sensitive   = true
}

variable "region" {
  description = "Linode region"
  type        = string
  default     = "us-ord" # Chicago
}

variable "instance_type" {
  description = "Linode instance plan"
  type        = string
  default     = "g6-standard-2" # 2 CPU, 4 GB RAM
}

variable "image_id" {
  description = "Linode image ID (from uploaded NixOS image, e.g. private/12345678)"
  type        = string
}

variable "authorized_keys" {
  description = "SSH public keys to inject into the instance"
  type        = list(string)
  default     = []
}

variable "volume_size" {
  description = "Size of the persistent block storage volume in GB"
  type        = number
  default     = 20
}

variable "root_pass" {
  description = "Root password for the Linode instance (required by API, but SSH key auth is used)"
  type        = string
  sensitive   = true
}
