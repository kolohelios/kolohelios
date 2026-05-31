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

      rustToolchain =
        pkgs:
        pkgs.rust-bin.stable."1.95.0".default.override {
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
              # Tailwind CSS compiler — invoked from wrangler.toml's
              # [build] step after `build-site` renders templates to
              # dist/. nixpkgs ships the standalone Go binary; no
              # Node toolchain required.
              pkgs.tailwindcss
              # `cue` is used by `build-site` to export
              # `data/work-history.cue` to JSON the templates iterate
              # over. Same tool the shaka schema/audit checks use,
              # just exposed here for the portfolio's build path.
              pkgs.cue
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
