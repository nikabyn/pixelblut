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
      # wayland
      # vulkan-loader
    ];

    wildStdenv = pkgs.useWildLinker pkgs.stdenv;
  in {
    packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
      pname = "pixelblut";
      version = "0.0.0";

      src = ./.;
      cargoLock.lockFile = ./Cargo.lock;
      cargoBuildFlags = [];

      doCheck = false;

      nativeBuildInputs = with pkgs; [
        pkg-config
        makeWrapper
      ];

      buildInputs = libraries;
    };

    devShells.${system}.default = pkgs.mkShell.override {stdenv = wildStdenv;} {
      nativeBuildInputs = with pkgs; [
        rustc
        cargo
        rustfmt
        clippy
        rust-analyzer
        perf

        pkg-config
      ];

      buildInputs = with pkgs; [
      ];
    };
  };
}
