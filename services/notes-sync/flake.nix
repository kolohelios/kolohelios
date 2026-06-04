{
  description = "notes-sync";

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

      # wasm32-unknown-unknown is added explicitly so `cargo check
      # --target wasm32-unknown-unknown` (the wasm-check recipe) and any
      # local `worker-build --release` invocation both find the stdlib.
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
              pkgs.pkg-config
              pkgs.openssl
            ]
            ++ (workflowPackages pkgs)
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;

            # `worker-build` is installed lazily via `cargo install` in
            # `wrangler.toml`'s build command (the workers-rs documented
            # idiom). Append `~/.cargo/bin` to PATH so the resulting
            # binary is visible to wrangler. Append (not prepend) because
            # GitHub Actions runners ship a rustup-managed `cargo` in
            # `~/.cargo/bin` that would otherwise shadow nix's cargo and
            # bypass the project toolchain entirely.
            shellHook = ''
              export PATH="$PATH:$HOME/.cargo/bin"
            '';
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
