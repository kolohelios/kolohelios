//! `shaka domain check` — drift detection for owned domains.
//!
//! Three checks per domain (NS-pair limited to cloudflare-managed
//! zones):
//!
//! - **Expiry** — WHOIS-reported registry expiry date; fail when fewer
//!   than `expiry_warn_days` (default 30, overridable per-domain in
//!   the CUE registry) remain.
//! - **Lock** — WHOIS must report `clientTransferProhibited`; absence
//!   means the domain can be transferred out without the registrar
//!   first contacting us. Always-on.
//! - **NS pair** — `dig +noall +authority NS <domain> @a.gtld-servers.net`
//!   parses the registry delegation; the sorted, lowercase,
//!   trailing-dot-stripped result must match the `ns_pair` declared
//!   in the per-domain CUE entry. CUE is the source of truth (per
//!   #200); the live dig is the drift signal.
//!
//! DNSSEC (DS-at-registry vs DNSKEY-from-CF) is intentionally
//! out-of-scope for v1 — no owned domain has `dnssec_enabled: true`
//! yet. Filed as a follow-up.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

/// Default expiry-warn threshold when a `#Domain` entry omits
/// `expiry_warn_days`.
pub const DEFAULT_EXPIRY_WARN_DAYS: i64 = 30;

#[derive(Debug, Deserialize, Clone)]
pub struct Domain {
    pub name: String,
    pub disposition: String,
    #[serde(default)]
    pub expiry_warn_days: Option<i64>,
    pub nameservers: String,
    // Surfaced in the deserialized record so future DNSSEC drift
    // checks (filed as a follow-up to #200) can switch on it without
    // a schema migration. Not read by the v1 checks.
    #[serde(default)]
    #[allow(dead_code)]
    pub dnssec_enabled: Option<bool>,
    #[serde(default)]
    pub ns_pair: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RegistryExport {
    #[serde(default)]
    domains: BTreeMap<String, Domain>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: &'static str,
    pub status: Status,
    pub detail: String,
}

/// Days since the Unix epoch for the current wall clock. Used as the
/// "now" anchor for expiry-window arithmetic. Truncates to whole days
/// so the result is stable for tests with a fixed-date anchor.
fn today_days_since_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86400) as i64)
        .unwrap_or(0)
}

/// Pure: parse the date prefix of an RFC-3339 timestamp
/// (`YYYY-MM-DDT...`) into days-since-Unix-epoch. Returns `None` for
/// malformed input or out-of-range components. Time-of-day is ignored
/// — the WHOIS expiry semantics treat midnight as the relevant
/// rollover, and per-second precision adds nothing to a 30-day warn
/// window.
pub fn parse_rfc3339_date_to_days(s: &str) -> Option<i64> {
    let date_part = s.trim().split('T').next()?;
    let mut parts = date_part.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// Howard Hinnant's `days_from_civil` algorithm
/// (<https://howardhinnant.github.io/date_algorithms.html#days_from_civil>).
/// Maps a proleptic-Gregorian (year, month, day) to days since the
/// Unix epoch (1970-01-01 = 0). Negative for dates before 1970.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = (y as i64) - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let m = m as u64;
    let d = d as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Pure: format days-since-epoch back to `YYYY-MM-DD`. Inverse of
/// `days_from_civil`. Used in user-facing output so the report shows
/// the parsed expiry date instead of an opaque day count.
pub fn format_days_as_date(days: i64) -> String {
    // Inverse algorithm from Hinnant's page.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn run(registry_dir: &Path, only: Option<&str>, expiry_warn_override: Option<i64>) {
    let domains = match load_registry(registry_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            std::process::exit(1);
        }
    };

    let selected: Vec<&Domain> = match only {
        Some(name) => match domains.iter().find(|d| d.name == name) {
            Some(d) => vec![d],
            None => {
                eprintln!(
                    "{RED}{BOLD}error:{RESET} domain {name:?} not found in {}",
                    registry_dir.display()
                );
                std::process::exit(1);
            }
        },
        None => domains.iter().collect(),
    };

    if selected.is_empty() {
        println!(
            "{YELLOW}{BOLD}no domains in registry{RESET} ({})",
            registry_dir.display()
        );
        return;
    }

    let now = today_days_since_epoch();
    let mut total_fail = 0usize;
    let mut total_skip = 0usize;
    let mut total_pass = 0usize;

    println!("{BOLD}shaka domain check{RESET}");
    println!("{DIM}├── registry: {}{RESET}", registry_dir.display());

    for (i, domain) in selected.iter().enumerate() {
        let last_domain = i == selected.len() - 1;
        let domain_branch = if last_domain {
            "└──"
        } else {
            "├──"
        };
        println!(
            "{domain_branch} {BOLD}{}{RESET} {DIM}({}){RESET}",
            domain.name, domain.disposition
        );

        let findings = check_one(domain, now, expiry_warn_override);
        let n = findings.len();
        for (j, f) in findings.iter().enumerate() {
            let leaf = if j == n - 1 { "└──" } else { "├──" };
            let stem = if last_domain { "    " } else { "│   " };
            let (label, color) = match f.status {
                Status::Pass => ("PASS", GREEN),
                Status::Fail => ("FAIL", RED),
                Status::Skip => ("SKIP", YELLOW),
            };
            println!(
                "{stem}{leaf} {color}{BOLD}[{label}]{RESET} {}: {}",
                f.check, f.detail
            );
            match f.status {
                Status::Pass => total_pass += 1,
                Status::Fail => total_fail += 1,
                Status::Skip => total_skip += 1,
            }
        }
    }

    println!();
    if total_fail > 0 {
        eprintln!(
            "{RED}{BOLD}domain check failed{RESET}: {total_fail} fail, {total_skip} skip, {total_pass} pass"
        );
        std::process::exit(1);
    }
    println!("{GREEN}{BOLD}domain check passed{RESET}: {total_skip} skip, {total_pass} pass");
}

fn check_one(domain: &Domain, now_days: i64, expiry_override: Option<i64>) -> Vec<Finding> {
    let mut out = Vec::new();
    let warn_days = expiry_override
        .or(domain.expiry_warn_days)
        .unwrap_or(DEFAULT_EXPIRY_WARN_DAYS);

    match run_whois(&domain.name) {
        Ok(whois) => {
            out.push(evaluate_expiry(&whois, now_days, warn_days));
            out.push(evaluate_lock(&whois));
        }
        Err(e) => {
            out.push(Finding {
                check: "expiry",
                status: Status::Fail,
                detail: format!("whois failed: {e}"),
            });
            out.push(Finding {
                check: "lock",
                status: Status::Fail,
                detail: format!("whois failed: {e}"),
            });
        }
    }

    if domain.nameservers == "cloudflare" {
        match (&domain.ns_pair, run_dig_ns(&domain.name)) {
            (Some(expected), Ok(observed)) => {
                out.push(evaluate_ns_pair(expected, &observed));
            }
            (None, _) => out.push(Finding {
                check: "ns_pair",
                status: Status::Fail,
                detail: "cloudflare-managed domain missing `ns_pair` in CUE".into(),
            }),
            (_, Err(e)) => out.push(Finding {
                check: "ns_pair",
                status: Status::Fail,
                detail: format!("dig failed: {e}"),
            }),
        }
    } else {
        out.push(Finding {
            check: "ns_pair",
            status: Status::Skip,
            detail: format!("nameservers = {:?}; NS check skipped", domain.nameservers),
        });
    }

    out
}

fn run_whois(domain: &str) -> Result<String, String> {
    let out = Command::new("whois")
        .arg(domain)
        .output()
        .map_err(|e| format!("spawn whois: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn run_dig_ns(domain: &str) -> Result<Vec<String>, String> {
    let out = Command::new("dig")
        .args(["+noall", "+authority", "NS", domain, "@a.gtld-servers.net"])
        .output()
        .map_err(|e| format!("spawn dig: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_ns_authority(&String::from_utf8_lossy(&out.stdout)))
}

/// Pure: parse `dig +noall +authority NS` output into a sorted,
/// lowercase, trailing-dot-stripped list of NS hostnames.
///
/// Each NS line looks like:
/// `kolohelios.com.\t172800\tIN\tNS\tnora.ns.cloudflare.com.`
/// We split on whitespace and take the 5th field, normalize, and sort
/// so the comparison is order-independent (TF emits sorted too).
pub fn parse_ns_authority(stdout: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // [zone, ttl, class, type, rdata]
        if parts.len() < 5 || parts[3] != "NS" {
            continue;
        }
        let host = parts[4].trim_end_matches('.').to_ascii_lowercase();
        if !host.is_empty() {
            out.push(host);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn evaluate_expiry(whois: &str, now_days: i64, warn_days: i64) -> Finding {
    match parse_whois_expiry(whois) {
        Some(expiry_days) => {
            let remaining = expiry_days - now_days;
            let expiry_str = format_days_as_date(expiry_days);
            if remaining < warn_days {
                Finding {
                    check: "expiry",
                    status: Status::Fail,
                    detail: format!(
                        "{remaining} days until expiry ({expiry_str}); warn threshold {warn_days}"
                    ),
                }
            } else {
                Finding {
                    check: "expiry",
                    status: Status::Pass,
                    detail: format!("{remaining} days until expiry ({expiry_str})"),
                }
            }
        }
        None => Finding {
            check: "expiry",
            status: Status::Fail,
            detail: "could not parse expiry date from whois output".into(),
        },
    }
}

fn evaluate_lock(whois: &str) -> Finding {
    if whois_has_lock(whois) {
        Finding {
            check: "lock",
            status: Status::Pass,
            detail: "clientTransferProhibited set".into(),
        }
    } else {
        Finding {
            check: "lock",
            status: Status::Fail,
            detail: "clientTransferProhibited is NOT set on this domain".into(),
        }
    }
}

fn evaluate_ns_pair(expected: &[String], observed: &[String]) -> Finding {
    // Normalize the CUE-declared values the same way we normalize dig
    // output (lowercase, trailing-dot-stripped) so the comparison is
    // string-equal regardless of how the entry was written.
    let mut normalized_expected: Vec<String> = expected
        .iter()
        .map(|s| s.trim_end_matches('.').to_ascii_lowercase())
        .collect();
    normalized_expected.sort();
    normalized_expected.dedup();

    if normalized_expected == observed {
        return Finding {
            check: "ns_pair",
            status: Status::Pass,
            detail: format!("registrar delegation matches CUE: {}", observed.join(", ")),
        };
    }
    Finding {
        check: "ns_pair",
        status: Status::Fail,
        detail: format!(
            "registrar delegation drift: expected {} but registrar reports {}",
            normalized_expected.join(", "),
            observed.join(", ")
        ),
    }
}

/// Pure: pull the expiry date out of a WHOIS payload, returned as days
/// since the Unix epoch.
///
/// Tries the canonical `Registry Expiry Date:` line first (RFC 9083
/// gTLD output), then falls back to `Registrar Registration Expiration
/// Date:` for registrars that emit only the latter. Returns `None` if
/// neither line parses.
pub fn parse_whois_expiry(whois: &str) -> Option<i64> {
    const KEYS: &[&str] = &[
        "Registry Expiry Date:",
        "Registrar Registration Expiration Date:",
    ];
    for line in whois.lines() {
        let trimmed = line.trim();
        for key in KEYS {
            if let Some(rest) = trimmed.strip_prefix(key) {
                if let Some(days) = parse_rfc3339_date_to_days(rest.trim()) {
                    return Some(days);
                }
            }
        }
    }
    None
}

/// Pure: detect the `clientTransferProhibited` lock in a WHOIS payload.
pub fn whois_has_lock(whois: &str) -> bool {
    whois
        .lines()
        .any(|line| line.contains("clientTransferProhibited"))
}

fn load_registry(dir: &Path) -> Result<Vec<Domain>, String> {
    let output = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(".")
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("cue export {}: {detail}", dir.display()));
    }
    let export: RegistryExport = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse cue export output: {e}"))?;
    Ok(export.domains.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/domain")
            .join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn parse_ns_authority_extracts_sorted_lowercase_hostnames() {
        let out = fixture("dig-ns/kolohelios-com.txt");
        let ns = parse_ns_authority(&out);
        assert_eq!(
            ns,
            vec![
                "kayden.ns.cloudflare.com".to_string(),
                "nora.ns.cloudflare.com".to_string(),
            ]
        );
    }

    #[test]
    fn parse_ns_authority_ignores_unrelated_lines() {
        let stdout = "\
;; AUTHORITY SECTION:
example.com.\t172800\tIN\tNS\tns1.example.com.
;; ADDITIONAL SECTION:
ns1.example.com.\t172800\tIN\tA\t1.2.3.4
";
        let ns = parse_ns_authority(stdout);
        assert_eq!(ns, vec!["ns1.example.com".to_string()]);
    }

    #[test]
    fn parse_ns_authority_empty_on_unrelated_input() {
        assert!(parse_ns_authority("").is_empty());
        assert!(parse_ns_authority("not relevant\n").is_empty());
    }

    #[test]
    fn parse_whois_expiry_extracts_registry_field() {
        let whois = fixture("whois/kolohelios-com.txt");
        let expiry = parse_whois_expiry(&whois).expect("expiry parses");
        // The fixture is captured 2026-05-26; expiry is 2027-03-26.
        assert_eq!(format_days_as_date(expiry), "2027-03-26");
    }

    #[test]
    fn parse_whois_expiry_returns_none_when_absent() {
        assert!(parse_whois_expiry("garbage line\nanother\n").is_none());
    }

    #[test]
    fn whois_has_lock_true_when_status_present() {
        let whois = fixture("whois/kolohelios-com.txt");
        assert!(whois_has_lock(&whois));
    }

    #[test]
    fn whois_has_lock_false_when_absent() {
        assert!(!whois_has_lock("Domain Status: ok"));
    }

    #[test]
    fn evaluate_expiry_passes_when_far_out() {
        let whois = fixture("whois/kolohelios-com.txt");
        // 2026-06-01: ~10 months before 2027-03-26 expiry; well over warn.
        let now = days_from_civil(2026, 6, 1);
        let f = evaluate_expiry(&whois, now, 30);
        assert_eq!(f.status, Status::Pass);
    }

    #[test]
    fn evaluate_expiry_fails_within_warn_window() {
        let whois = fixture("whois/kolohelios-com.txt");
        // 2027-03-15: 11 days before 2027-03-26 expiry; default warn fires.
        let now = days_from_civil(2027, 3, 15);
        let f = evaluate_expiry(&whois, now, 30);
        assert_eq!(f.status, Status::Fail);
        assert!(f.detail.contains("11 days"), "got: {}", f.detail);
    }

    #[test]
    fn evaluate_ns_pair_passes_when_normalized_equal() {
        let expected: Vec<String> = vec![
            "Kayden.ns.cloudflare.com.".into(),
            "nora.ns.cloudflare.com.".into(),
        ];
        let observed: Vec<String> = vec![
            "kayden.ns.cloudflare.com".into(),
            "nora.ns.cloudflare.com".into(),
        ];
        let f = evaluate_ns_pair(&expected, &observed);
        assert_eq!(f.status, Status::Pass);
    }

    #[test]
    fn evaluate_ns_pair_fails_on_drift() {
        let expected: Vec<String> = vec![
            "kayden.ns.cloudflare.com.".into(),
            "nora.ns.cloudflare.com.".into(),
        ];
        let observed: Vec<String> = vec![
            "kayden.ns.cloudflare.com".into(),
            "stranger.ns.example.com".into(),
        ];
        let f = evaluate_ns_pair(&expected, &observed);
        assert_eq!(f.status, Status::Fail);
        assert!(f.detail.contains("drift"), "got: {}", f.detail);
    }
}
