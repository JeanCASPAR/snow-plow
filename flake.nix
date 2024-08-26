{
  description = "Allows to update all tracked flakes of your machine in one go";
  inputs = {
    nixpkgs.url = github:NixOS/nixpkgs/nixpkgs-unstable;
    utils.url = github:numtide/flake-utils;
    rust-overlay = {
      url = github:oxalica/rust-overlay;
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk.url = github:nix-community/naersk;
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
          root = ./.;
          nativeBuildInputs = [ pkgs.installShellFiles ];
          # man files and shell completions
          postInstall = ''
            mkdir $out/artifacts
            cd $out/artifacts

            $out/bin/snow-plow gen-man
            installManPage ./*.1

            cd $out/share
            rm -r $out/artifacts

            $out/bin/snow-plow gen-completion bash
            $out/bin/snow-plow gen-completion fish
            $out/bin/snow-plow gen-completion zsh
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
