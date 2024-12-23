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
        toolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" ];
        };
        naersk' = pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };
      in {
        defaultPackage = naersk'.buildPackage {
          pname = "snow-plow";
          src = ./.;
          nativeBuildInputs = [ pkgs.installShellFiles ];
          # man files and shell completions
          postInstall = ''
            mkdir $out/artifacts
            cd $out/artifacts

            $out/bin/snow-plow gen-man

            $out/bin/snow-plow gen-completion bash
            $out/bin/snow-plow gen-completion fish
            $out/bin/snow-plow gen-completion zsh

            cd $out

            installManPage $out/artifacts/*.1
            installShellCompletion \
              --cmd snow-plow \
              --bash $out/artifacts/snow-plow.bash \
              --fish $out/artifacts/snow-plow.fish \
              --zsh $out/artifacts/_snow-plow

            rm -r $out/artifacts
          '';
        };
        defaultApp = utils.lib.mkApp {
          drv = self.defaultPackage.${system};
        };
        devShell = with pkgs; mkShell {
          packages = [
            toolchain
            cargo
            rustfmt
            rustPackages.clippy
            rust-analyzer
          ];
        };
      });
}
