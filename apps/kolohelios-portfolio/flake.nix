{
  description = "kolohelios-portfolio";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      kolohelios-nix,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      inherit (kolohelios-nix.lib) supportedSystems workflowPackages;

      forEachSupportedSystem =
        f:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              config.allowUnfree = true;
              overlays = [ rust-overlay.overlays.default ];
            };
          }
        );

      # Diagnostic pin for #403. `wrangler deploy` was failing in CI
      # because `rustc --print sysroot` returned the April-29 nightly
      # toolchain even though our dev-shell drv closure only references
      # the May-11 nightly (verified locally with
      # `nix-store -q --requisites` on the linux drv). PR #414 switched
      # `nightly.latest.default.override` to `selectLatestNightlyWith`,
      # but that turned out to be a no-op against the currently pinned
      # `rust-overlay` rev — both expressions produce the same
      # `outPath`. So the April-29 toolchain is entering the CI
      # environment from somewhere outside our flake's declared
      # closure; the source is unknown.
      #
      # Pinning the date explicitly removes rust-overlay's
      # date-selection logic from the equation. If CI now passes, the
      # previous `.latest`-driven setup was non-deterministic in CI and
      # this pin is a workable fix. If CI still fails the same way, the
      # April-29 toolchain enters from outside our flake entirely
      # (e.g. a runner-level rustup install we aren't suppressing) and
      # the investigation moves off the toolchain expression.
      #
      # `wasm32-unknown-unknown` stays in `targets` so `cargo check
      # --target wasm32-unknown-unknown` (the wasm-check recipe) and
      # any local `worker-build --release` invocation both find the
      # stdlib regardless.
      rustToolchain =
        pkgs:
        pkgs.rust-bin.nightly."2026-05-11".default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools-preview"
          ];
          targets = [ "wasm32-unknown-unknown" ];
        };
    in
    {
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              (rustToolchain pkgs)
              pkgs.wrangler
              # `cargo install worker-build` (lazily invoked by
              # wrangler.toml's `[build]` command) pulls in
              # `openssl-sys`, which needs system openssl headers +
              # pkg-config to compile natively. Locally invisible
              # because the prebuilt `worker-build` is cached in
              # `~/.cargo/bin/`; CI starts fresh, so the install
              # actually runs and the missing headers fail the build.
              # #333 tracks packaging `worker-build` as a nix
              # derivation, which would let us drop both of these.
              pkgs.pkg-config
              pkgs.openssl
            ]
            ++ (workflowPackages pkgs)
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;

            # `worker-build` is installed lazily via `cargo install` in
            # `wrangler.toml`'s build command (the workers-rs documented
            # idiom — see #333 for whether to package it as a nix
            # derivation instead). Append `~/.cargo/bin` to PATH so the
            # resulting binary is visible to wrangler. Append (not
            # prepend) because GitHub Actions runners ship a
            # rustup-managed `cargo` in `~/.cargo/bin` that would
            # otherwise shadow nix's cargo and bypass the project
            # toolchain entirely.
            shellHook = ''
              export PATH="$PATH:$HOME/.cargo/bin"
            '';
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
