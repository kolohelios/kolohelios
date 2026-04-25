output "devbox_ip" {
  description = "Public IPv4 address of the devbox"
  value       = linode_instance.devbox.ipv4
}

output "devbox_id" {
  description = "Linode instance ID"
  value       = linode_instance.devbox.id
}

output "volume_filesystem_path" {
  description = "Device path of the attached block storage volume"
  value       = linode_volume.persist.filesystem_path
}
