{
  description = "claude-hooks";

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
        pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools-preview"
          ];
        };
    in
    {
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              (rustToolchain pkgs)
            ]
            ++ (workflowPackages pkgs)
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;
          };
        }
      );

      # Consumed by `infra/home/modules/common.nix` so the binary is on
      # PATH for every claude session, not just inside the kolohelios
      # checkout. Build deps stay minimal — no `rust-overlay` toolchain
      # here, just nixpkgs's stable rustc + cargo, since the source has
      # no nightly-only features.
      packages = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "claude-hooks";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            # Tests touch `gh` and `jj` via `Command::new`, which works
            # in the build sandbox only because the test cases that
            # exercise `search_open_issues` / `detect_repo` aren't
            # reached — they're called from `pre_issue_create` at
            # runtime, not from any `#[test]` fn. Keep `checkPhase` on
            # so the parser unit tests run during `nix build`.
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
