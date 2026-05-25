# zero-branch-coverage fixture

Rust project with `coverage.branch.fail: 0` and `line.fail: 1` — should pass audit.
`cargo-llvm-cov --branch` doesn't count `match` arms, so fresh scaffolds can't hit
any branch threshold; branch=0 is allowed as long as line is gated.
