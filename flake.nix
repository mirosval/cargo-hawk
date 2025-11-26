{
  description = "Cargo Hawk";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };
  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        inherit (pkgs) lib;
        craneLib = crane.mkLib pkgs;
        snapFilter = path: _type: builtins.match ".*snap$" path != null;
        snapOrCargo = path: type: (snapFilter path type) || (craneLib.filterCargoSources path type);
        src = lib.cleanSourceWith {
          src = ./.;
          filter = snapOrCargo;
          name = "source";
        };
        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = [ ] ++ lib.optionals pkgs.stdenv.isDarwin [ ];

        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        cargo-hawk = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );
      in
      {
        checks = {
          inherit cargo-hawk;

          cargo-hawk-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny-warnings";
            }
          );

          cargo-hawk-fmt = craneLib.cargoFmt {
            inherit src;
          };

          cargo-hawk-test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );
        };

        packages.default = cargo-hawk;

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          packages = with pkgs; [
            cargo-hawk
            cargo-insta
            cargo-outdated
            cargo-udeps
            rust-analyzer
            tailspin
          ];
        };
      }
    );
}
