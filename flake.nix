{
  description = "Allows to update all tracked flakes of your machine in one go";
  inputs = {
    nixpkgs.url = github:NixOS/nixpkgs/nixpkgs-unstable;
    utils.url = github:numtide/flake-utils;
    rust-overlay = {
      url = github:oxalica/rust-overlay;
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = github:nix-community/naersk;
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, utils, naersk, rust-overlay }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
          ];
        };
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" ];
        };
        naerskLib = naersk.lib.${system}.override {
          cargo = rust;
          rustc = rust;
        };
      in {
        defaultPackage = naerskLib.buildPackage {
          pname = "snow-plow";
          src = ./.;
          nativeBuildInputs = [ pkgs.installShellFiles ];
          # man files and shell completions
          postBuild = ''
            mkdir ./artifacts
            cd ./artifacts

            echo $(ls ../target/release)
            cat $cargo_build_output_json
            ../target/release/snow-plow gen-man

            ../target/release/snow-plow gen-completion bash
            ../target/release/snow-plow gen-completion fish
            ../target/release/snow-plow gen-completion zsh
          '';
          postInstall = ''
            installManPage $out/artifacts/*.1
            installShellCompletion \
              --bash $out/artifacts/snow-plow.bash
              --fish $out/artifacts/snow-plow.fish
              --zsh $out/artifacts/_snow-plow
            rm $out/artifacts
          '';
        };
        defaultApp = utils.lib.mkApp {
          drv = self.defaultPackage.${system};
        };
        devShell = with pkgs; mkShell {
          packages = [
            rust
            cargo
            rustfmt
            rustPackages.clippy
            rust-analyzer
          ];
        };
      });
}
