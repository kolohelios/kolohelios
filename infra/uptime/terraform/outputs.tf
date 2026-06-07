output "monitor_ids" {
  description = "Better Stack monitor IDs, for cross-referencing in the dashboard or future tooling."
  value = {
    apex_http = betteruptime_monitor.apex_http.id
    apex_dns  = betteruptime_monitor.apex_dns.id
  }
}
