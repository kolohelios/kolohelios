# pollen-alert

Cloudflare Workers `cron` that fires nightly, scores overnight
pollen-trap risk for the configured location, and sends a `Pushover`
notification when conditions cross threshold — prompting closing the
windows.

Scaffold only at this stage. Scoring, forecasting, notification, and
the `cron` entrypoint land in follow-ups; the full `README` lands with
#410. See #411 for the project plan.
