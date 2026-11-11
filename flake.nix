{
  description = "TODO";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = {nixpkgs, ...}: let
    system = "x86_64-linux";

    pkgs = import nixpkgs {
      inherit system;
    };

    libraries = with pkgs; [
    ];
  in {
    packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
      pname = "pixelblut";
      version = "0.0.0";

      src = ./.;
      cargoLock.lockFile = ./Cargo.lock;
      cargoBuildFlags = [];

      doCheck = false;

      nativeBuildInputs = with pkgs; [
        makeWrapper
      ];

      buildInputs = libraries;
    };

    devShells.${system}.default = pkgs.mkShell {
      nativeBuildInputs = with pkgs; [
        cargo
        rustfmt
        clippy
        rust-analyzer
        perf
        valgrind-light
      ];

      buildInputs = with pkgs; [
      ];
    };
  };
}
