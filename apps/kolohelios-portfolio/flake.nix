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

      # `selectLatestNightlyWith` picks the latest nightly date for which
      # *every* requested component is available — base profile,
      # extensions, AND the `wasm32-unknown-unknown` rust-std. The
      # naive `rust-bin.nightly.latest.default.override` resolves each
      # component's "latest" independently, so when upstream lands a
      # newer wasm rust-std before a newer default profile (or vice
      # versa), the combined toolchain ends up with `rustc` from one
      # date and the wasm stdlib from another. `rustc --print sysroot`
      # then points at the base toolchain (no wasm), and worker-build's
      # fresh-install probe fails with "wasm32-unknown-unknown target
      # not found in sysroot". See #403 and run
      # https://github.com/kolohelios/kolohelios/actions/runs/25882388549.
      rustToolchain =
        pkgs:
        pkgs.rust-bin.selectLatestNightlyWith (
          toolchain:
          toolchain.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "llvm-tools-preview"
            ];
            targets = [ "wasm32-unknown-unknown" ];
          }
        );
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
