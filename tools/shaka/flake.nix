{
  description = "shaka — build tooling for kolohelios";

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
      packages = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "shaka";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            # Integration tests shell out to external tools: tests/schema.rs
            # uses `cue vet`; tests/workspace.rs uses `jj` (which itself
            # invokes `git` for colocated repos). Make all three available
            # during the check phase so `nix flake check` (and CI's `shaka
            # preflight`) can run them in the build sandbox.
            nativeCheckInputs = [
              pkgs.cue
              pkgs.jujutsu
              pkgs.git
            ];
            # Bake shaka's runtime deps onto the wrapped binary's PATH so
            # `nix run ./tools/shaka -- <subcmd>` works without a dev shell
            # — that's how non-preflight CI workflows (auto-rebase,
            # kolohelios-nix bump) skip the heavy Rust toolchain fetch.
            # Set deliberately to the everyday subset; less-common shaka
            # subcommands (object-store, preflight) still want the dev
            # shell, which carries awscli2/tofu/actionlint/vale/typos.
            nativeBuildInputs = [ pkgs.makeWrapper ];
            postFixup = ''
              wrapProgram $out/bin/shaka \
                --prefix PATH : ${
                  pkgs.lib.makeBinPath [
                    pkgs.jujutsu
                    pkgs.git
                    pkgs.gh
                    pkgs.nix
                    pkgs.cue
                    pkgs.just
                    pkgs.jq
                  ]
                }
            '';
          };
        }
      );

      apps = forEachSupportedSystem (
        { system, ... }:
        {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/shaka";
          };
        }
      );

      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              (rustToolchain pkgs)
              # actionlint runs in `shaka preflight` against
              # `.github/workflows/`; shellcheck is its dependency for
              # linting embedded `run:` scripts.
              pkgs.actionlint
              pkgs.shellcheck
              # awscli2 powers `shaka object-store ns audit` (list bucket
              # prefixes via the S3-compatible API on Linode Object
              # Storage). Audits skip-gracefully when AWS creds are
              # absent so CI without secrets still passes preflight.
              pkgs.awscli2
            ]
            ++ (workflowPackages pkgs)
            # cargo-llvm-cov is needed by `just coverage`. Nixpkgs marks
            # it broken on darwin (rust profiler runtime not in nixpkgs's
            # darwin rustc); developers on darwin install it via
            # `cargo install cargo-llvm-cov`.
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
