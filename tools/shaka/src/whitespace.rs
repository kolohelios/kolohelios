use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::jj;
use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

const BOM: &[u8] = b"\xef\xbb\xbf";
const BINARY_PROBE: usize = 8192;

#[derive(Subcommand)]
pub enum WhitespaceCommand {
    /// Report whitespace and line-ending issues across tracked files
    Check {
        /// Limit the scan to these paths (default: current directory)
        #[arg(default_value = ".")]
        paths: Vec<String>,
    },
    /// Rewrite tracked files in place to fix whitespace and line-ending issues
    Fix {
        #[arg(default_value = ".")]
        paths: Vec<String>,
    },
}

pub fn run(cmd: WhitespaceCommand) {
    let result = match cmd {
        WhitespaceCommand::Check { paths } => check(&paths),
        WhitespaceCommand::Fix { paths } => fix(&paths),
    };
    if !result {
        std::process::exit(1);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Issue {
    Bom,
    Crlf { lines: Vec<usize> },
    TrailingWhitespace { lines: Vec<usize> },
    MissingTrailingNewline,
    MultipleTrailingNewlines,
}

impl Issue {
    fn label(&self) -> &'static str {
        match self {
            Issue::Bom => "BOM",
            Issue::Crlf { .. } => "CRLF line endings",
            Issue::TrailingWhitespace { .. } => "trailing whitespace",
            Issue::MissingTrailingNewline => "missing trailing newline",
            Issue::MultipleTrailingNewlines => "multiple trailing newlines",
        }
    }

    fn lines(&self) -> Option<&[usize]> {
        match self {
            Issue::Crlf { lines } | Issue::TrailingWhitespace { lines } => Some(lines),
            _ => None,
        }
    }
}

fn check(paths: &[String]) -> bool {
    let files = match list_tracked_files(paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            return false;
        }
    };

    let mut scanned = 0usize;
    let mut bad_files = 0usize;
    let mut total_issues = 0usize;
    let mut first = true;

    for file in &files {
        let bytes = match std::fs::read(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if is_binary(&bytes) {
            continue;
        }
        scanned += 1;
        let issues = scan_bytes(&bytes);
        if issues.is_empty() {
            continue;
        }
        bad_files += 1;
        total_issues += issues.len();
        if first {
            first = false;
        }
        print_file_issues(file, &issues);
    }

    if bad_files == 0 {
        println!("{GREEN}{BOLD}whitespace check passed{RESET} ({scanned} text files scanned)");
        true
    } else {
        eprintln!(
            "\n{RED}{BOLD}whitespace check failed{RESET}: {total_issues} issue(s) in {bad_files} file(s)"
        );
        eprintln!("{YELLOW}Run `shaka whitespace fix` to repair.{RESET}");
        false
    }
}

fn fix(paths: &[String]) -> bool {
    let files = match list_tracked_files(paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{RED}{BOLD}error:{RESET} {e}");
            return false;
        }
    };

    let mut changed = 0usize;
    let mut scanned = 0usize;

    for file in &files {
        let bytes = match std::fs::read(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if is_binary(&bytes) {
            continue;
        }
        scanned += 1;
        let new_bytes = fix_bytes(&bytes);
        if new_bytes != bytes {
            if let Err(e) = std::fs::write(file, &new_bytes) {
                eprintln!("{RED}{BOLD}error:{RESET} writing {}: {e}", file.display());
                return false;
            }
            println!("  {GREEN}{BOLD}fixed{RESET}    {}", file.display());
            changed += 1;
        }
    }

    println!("\n{GREEN}{BOLD}whitespace fix:{RESET} rewrote {changed} of {scanned} files");
    true
}

fn print_file_issues(file: &Path, issues: &[Issue]) {
    println!("\n{BOLD}{}{RESET}", file.display());
    for issue in issues {
        match issue.lines() {
            Some(lines) if !lines.is_empty() => {
                let preview: Vec<String> = lines.iter().take(5).map(|l| l.to_string()).collect();
                let suffix = if lines.len() > 5 {
                    format!(" (+{} more)", lines.len() - 5)
                } else {
                    String::new()
                };
                println!(
                    "  {RED}{}{RESET} {DIM}lines {}{}{RESET}",
                    issue.label(),
                    preview.join(", "),
                    suffix
                );
            }
            _ => {
                println!("  {RED}{}{RESET}", issue.label());
            }
        }
    }
}

/// Scan the bytes of a single file and return the issues it contains.
pub fn scan_bytes(content: &[u8]) -> Vec<Issue> {
    let mut issues = Vec::new();

    let body = if content.starts_with(BOM) {
        issues.push(Issue::Bom);
        &content[BOM.len()..]
    } else {
        content
    };

    let mut crlf_lines = Vec::new();
    let mut tw_lines = Vec::new();
    for (idx, line) in body.split(|&b| b == b'\n').enumerate() {
        let lineno = idx + 1;
        let (logical, has_cr) = strip_cr(line);
        if has_cr {
            crlf_lines.push(lineno);
        }
        if logical.last().is_some_and(|&b| b == b' ' || b == b'\t') {
            tw_lines.push(lineno);
        }
    }
    // `split` produces N+1 segments for N newlines. The final segment (which
    // is empty when the file ends with `\n`) is not a real line — drop it
    // from per-line diagnostics so we don't report the "phantom" line after a
    // trailing newline.
    if body.last() == Some(&b'\n') {
        let phantom = body.split(|&b| b == b'\n').count();
        if let Some(p) = tw_lines.last() {
            if *p == phantom {
                tw_lines.pop();
            }
        }
        if let Some(p) = crlf_lines.last() {
            if *p == phantom {
                crlf_lines.pop();
            }
        }
    }
    if !crlf_lines.is_empty() {
        issues.push(Issue::Crlf { lines: crlf_lines });
    }
    if !tw_lines.is_empty() {
        issues.push(Issue::TrailingWhitespace { lines: tw_lines });
    }

    let trailing_nl = trailing_newline_count(body);
    if !body.is_empty() {
        if trailing_nl == 0 {
            issues.push(Issue::MissingTrailingNewline);
        } else if trailing_nl >= 2 {
            issues.push(Issue::MultipleTrailingNewlines);
        }
    }

    issues
}

/// Rewrite the bytes of a single file to remove all whitespace issues.
pub fn fix_bytes(content: &[u8]) -> Vec<u8> {
    let body = if content.starts_with(BOM) {
        &content[BOM.len()..]
    } else {
        content
    };

    let cleaned: Vec<Vec<u8>> = body
        .split(|&b| b == b'\n')
        .map(|line| {
            let (no_cr, _) = strip_cr(line);
            let end = no_cr
                .iter()
                .rposition(|&b| b != b' ' && b != b'\t')
                .map(|i| i + 1)
                .unwrap_or(0);
            no_cr[..end].to_vec()
        })
        .collect();

    let mut joined: Vec<u8> = Vec::new();
    for (i, line) in cleaned.iter().enumerate() {
        if i > 0 {
            joined.push(b'\n');
        }
        joined.extend_from_slice(line);
    }

    if joined.is_empty() {
        return joined;
    }

    let trim_end = joined
        .iter()
        .rposition(|&b| b != b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut out = joined[..trim_end].to_vec();

    if trim_end > 0 {
        out.push(b'\n');
    } else if joined.len() == 1 {
        // Original was a single `\n` — preserve it (one trailing newline, no
        // content). Anything longer that's all-newlines collapses to empty.
        out.push(b'\n');
    }

    out
}

fn strip_cr(line: &[u8]) -> (&[u8], bool) {
    if line.last() == Some(&b'\r') {
        (&line[..line.len() - 1], true)
    } else {
        (line, false)
    }
}

fn trailing_newline_count(body: &[u8]) -> usize {
    body.iter().rev().take_while(|&&b| b == b'\n').count()
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_PROBE).any(|&b| b == 0)
}

// Enumerate via jj when in a jj repo so the listing reflects the calling
// workspace's `@`: `git ls-files` reads the colocated index, which is
// pinned to the primary workspace and never sees files added in a sibling
// workspace's `@` (issue #210). In a plain git checkout (no jj repo —
// e.g. a GitHub Actions `actions/checkout`) there's no `@` to honor, so
// fall back to `git ls-files`; it's equivalent there because the index
// matches the checked-out tree (issue #902).
fn list_tracked_files(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let owned: Vec<String>;
    let arg_slice: &[String] = if paths.is_empty() {
        owned = vec![".".to_string()];
        &owned
    } else {
        paths
    };
    if jj::in_repo() {
        jj::file_list("@", arg_slice)
            .map(|files| files.into_iter().map(PathBuf::from).collect())
            .map_err(|e| e.to_string())
    } else {
        git_list_tracked_files(arg_slice)
    }
}

// `git ls-files` fallback for git-only checkouts. Mirrors `jj file list`'s
// output: tracked paths under the given pathspecs, relative to `cwd`.
fn git_list_tracked_files(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut args: Vec<&str> = vec!["ls-files", "--"];
    for p in paths {
        args.push(p.as_str());
    }
    let output = std::process::Command::new("git")
        .args(&args)
        .output()
        .map_err(|e| format!("failed to spawn git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git ls-files: {}", stderr.trim()));
    }
    Ok(parse_git_ls_files(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_git_ls_files(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(bytes: &[u8]) -> Vec<Issue> {
        scan_bytes(bytes)
    }

    #[test]
    fn parse_git_ls_files_drops_blank_lines() {
        let out = "src/main.rs\nCargo.toml\n\nbin/shaka\n";
        assert_eq!(
            parse_git_ls_files(out),
            vec![
                PathBuf::from("src/main.rs"),
                PathBuf::from("Cargo.toml"),
                PathBuf::from("bin/shaka"),
            ]
        );
    }

    #[test]
    fn parse_git_ls_files_empty_is_empty() {
        assert!(parse_git_ls_files("").is_empty());
    }

    #[test]
    fn empty_file_has_no_issues() {
        assert_eq!(issues(b""), vec![]);
    }

    #[test]
    fn single_newline_file_passes() {
        assert_eq!(issues(b"\n"), vec![]);
    }

    #[test]
    fn well_formed_file_passes() {
        assert_eq!(issues(b"hello\nworld\n"), vec![]);
    }

    #[test]
    fn missing_trailing_newline_is_flagged() {
        assert_eq!(issues(b"hello"), vec![Issue::MissingTrailingNewline]);
    }

    #[test]
    fn multiple_trailing_newlines_flagged() {
        assert_eq!(issues(b"hello\n\n"), vec![Issue::MultipleTrailingNewlines]);
        assert_eq!(
            issues(b"hello\n\n\n\n"),
            vec![Issue::MultipleTrailingNewlines]
        );
    }

    #[test]
    fn trailing_space_is_flagged_with_line_number() {
        assert_eq!(
            issues(b"hello \nworld\n"),
            vec![Issue::TrailingWhitespace { lines: vec![1] }]
        );
    }

    #[test]
    fn trailing_tab_is_flagged() {
        assert_eq!(
            issues(b"hello\t\n"),
            vec![Issue::TrailingWhitespace { lines: vec![1] }]
        );
    }

    #[test]
    fn crlf_is_flagged_with_line_number() {
        assert_eq!(
            issues(b"hello\r\nworld\n"),
            vec![Issue::Crlf { lines: vec![1] }]
        );
    }

    #[test]
    fn bom_is_flagged() {
        let mut content = b"\xef\xbb\xbf".to_vec();
        content.extend_from_slice(b"hello\n");
        assert_eq!(issues(&content), vec![Issue::Bom]);
    }

    #[test]
    fn bom_is_stripped_before_per_line_checks() {
        // BOM is followed by content without a trailing newline.
        let mut content = b"\xef\xbb\xbf".to_vec();
        content.extend_from_slice(b"hello");
        let result = issues(&content);
        assert!(result.contains(&Issue::Bom));
        assert!(result.contains(&Issue::MissingTrailingNewline));
    }

    #[test]
    fn multiple_issues_are_collected() {
        // Trailing space on line 1, CRLF on line 1, no trailing newline.
        assert_eq!(
            issues(b"hello \r\nworld"),
            vec![
                Issue::Crlf { lines: vec![1] },
                Issue::TrailingWhitespace { lines: vec![1] },
                Issue::MissingTrailingNewline,
            ]
        );
    }

    #[test]
    fn crlf_line_numbers_track_actual_lines() {
        assert_eq!(
            issues(b"a\nb\r\nc\r\n"),
            vec![Issue::Crlf { lines: vec![2, 3] }]
        );
    }

    #[test]
    fn phantom_trailing_line_is_not_reported() {
        // Without the phantom-line filter, a trailing-newline file would
        // record a spurious "line N+1" with empty content. Verify lines stay
        // bounded by real content lines.
        let result = issues(b"hello \n");
        assert_eq!(result, vec![Issue::TrailingWhitespace { lines: vec![1] }]);
    }

    #[test]
    fn fix_empty_stays_empty() {
        assert_eq!(fix_bytes(b""), b"");
    }

    #[test]
    fn fix_well_formed_is_unchanged() {
        let input = b"hello\nworld\n";
        assert_eq!(fix_bytes(input), input);
    }

    #[test]
    fn fix_adds_missing_trailing_newline() {
        assert_eq!(fix_bytes(b"hello"), b"hello\n");
    }

    #[test]
    fn fix_collapses_multiple_trailing_newlines() {
        assert_eq!(fix_bytes(b"hello\n\n\n"), b"hello\n");
    }

    #[test]
    fn fix_strips_trailing_whitespace_per_line() {
        assert_eq!(fix_bytes(b"hello \nworld\t\n"), b"hello\nworld\n");
    }

    #[test]
    fn fix_normalizes_crlf_to_lf() {
        assert_eq!(fix_bytes(b"hello\r\nworld\r\n"), b"hello\nworld\n");
    }

    #[test]
    fn fix_strips_bom() {
        let mut input = b"\xef\xbb\xbf".to_vec();
        input.extend_from_slice(b"hello\n");
        assert_eq!(fix_bytes(&input), b"hello\n");
    }

    #[test]
    fn fix_handles_combined_issues() {
        let mut input = b"\xef\xbb\xbf".to_vec();
        input.extend_from_slice(b"hello \r\nworld\t\r\n\n\n");
        assert_eq!(fix_bytes(&input), b"hello\nworld\n");
    }

    #[test]
    fn fix_preserves_single_newline_only_file() {
        assert_eq!(fix_bytes(b"\n"), b"\n");
    }

    #[test]
    fn fix_collapses_pure_newline_file_to_empty() {
        assert_eq!(fix_bytes(b"\n\n\n"), b"");
    }

    #[test]
    fn fix_collapses_whitespace_only_file_to_empty() {
        // All lines are whitespace-only; after trim they're empty; output is
        // the empty file (consistent with the "whitespace-only file is
        // pathological" decision).
        assert_eq!(fix_bytes(b"   \n\t\n"), b"");
    }

    #[test]
    fn fix_is_idempotent_on_check_passing_inputs() {
        for input in [
            &b""[..],
            &b"\n"[..],
            &b"hello\n"[..],
            &b"hello\nworld\n"[..],
            &b"a\n\nb\n"[..], // internal blank line preserved
        ] {
            assert!(
                scan_bytes(input).is_empty(),
                "input itself should pass: {:?}",
                input
            );
            assert_eq!(fix_bytes(input), input, "fix changed a passing input");
        }
    }

    #[test]
    fn fix_output_always_passes_check() {
        for input in [
            &b"hello"[..],
            &b"hello \n"[..],
            &b"hello\r\n"[..],
            &b"hello\n\n\n"[..],
            &b"\xef\xbb\xbfhello\n"[..],
            &b"\xef\xbb\xbfhello \r\n\n"[..],
        ] {
            let fixed = fix_bytes(input);
            assert!(
                scan_bytes(&fixed).is_empty(),
                "fix({:?}) = {:?} still has issues",
                input,
                fixed
            );
        }
    }

    #[test]
    fn binary_detected_by_nul_byte() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"abc\ndef\n"));
        assert!(!is_binary(b""));
    }

    #[test]
    fn binary_probe_capped_at_first_chunk() {
        // A NUL beyond the probe window is not detected; we accept this as a
        // documented heuristic limit.
        let mut data = vec![b'a'; BINARY_PROBE];
        data.push(0);
        assert!(!is_binary(&data));
    }

    #[test]
    fn internal_blank_lines_are_preserved() {
        assert_eq!(fix_bytes(b"a\n\nb\n"), b"a\n\nb\n");
        assert_eq!(scan_bytes(b"a\n\nb\n"), vec![]);
    }
}
