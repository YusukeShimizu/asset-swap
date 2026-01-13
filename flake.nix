{
  description = "LN→Liquid swap repository (dev env via Nix Flakes)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        textlint = pkgs.writeShellScriptBin "textlint" ''
          export NODE_PATH="${pkgs.textlint}/lib/node_modules:${pkgs.textlint-rule-preset-ja-technical-writing}/lib/node_modules:${pkgs.textlint-rule-preset-ja-spacing}/lib/node_modules:${pkgs.textlint-rule-prh}/lib/node_modules:$NODE_PATH"
          exec "${pkgs.textlint}/bin/textlint" "$@"
        '';

        # Vale uses the external `mdx2vast` converter when linting MDX.
        # https://vale.sh/docs/formats/mdx
        #
        # We package it via Nix (instead of `npx`) because Vale invokes `mdx2vast`
        # repeatedly and concurrent `npx` executions are error-prone in CI.
        mdx2vast = pkgs.buildNpmPackage rec {
          pname = "mdx2vast";
          version = "0.3.0";

          src = pkgs.fetchFromGitHub {
            owner = "jdkato";
            repo = "mdx2vast";
            rev = "v${version}";
            hash = "sha256-ICutpTV09tt6Pg+PDm0qL+yykMRd6vWR8h9nQyJlzIM=";
          };

          npmDepsHash = "sha256-KE3IzLDV8ICZ9ZlXRw0g2oM8mML5q2IvLVYWD45+f1o=";

          # This package is a CLI tool and doesn't require a build step.
          dontNpmBuild = true;
        };

        ldkServer = pkgs.rustPlatform.buildRustPackage rec {
          pname = "ldk-server";
          version = "0.0.0-f3eaacd";

          src = pkgs.fetchFromGitHub {
            owner = "lightningdevkit";
            repo = "ldk-server";
            rev = "f3eaacd327d40fc8ee3fd7f6fbaccb04fa077434";
            hash = "sha256-YuPSjMVHw+RrS25c6BVQ8aROSGwIBuKMut8ycupNXcs=";
          };

          cargoLock = {
            lockFile = "${src}/Cargo.lock";
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            protobuf
          ];

          buildInputs = with pkgs; [
            openssl
          ];

          cargoBuildFlags = [
            "--package"
            "ldk-server"
            "--bin"
            "ldk-server"
          ];

          doCheck = false;
        };

        electrsLiquid = pkgs.rustPlatform.buildRustPackage rec {
          pname = "electrs-liquid";
          version = "0.4.1-e60ca89";

          src = pkgs.fetchFromGitHub {
            owner = "blockstream";
            repo = "electrs";
            rev = "e60ca890959b2cb9b62d5253ffa0cf4b25b144eb";
            hash = "sha256-2z/cZcCg62tAd/a3qIVuiPZYruFQk7SwQYPTD5CnPek=";
          };

          cargoLock = {
            lockFile = "${src}/Cargo.lock";
            outputHashes = {
              "electrum-client-0.8.0" = "sha256-HDRdGS7CwWsPXkA1HdurwrVu4lhEx0Ay8vHi08urjZ0=";
              "electrumd-0.1.0" = "sha256-QsoMD2uVDEITuYmYItfP6BJCq7ApoRztOCs7kdeRL9Y=";
              "jsonrpc-0.12.0" = "sha256-lSNkkQttb8LnJej4Vfe7MrjiNPOuJ5A6w5iLstl9O1k=";
            };
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          cargoBuildFlags = [
            "--features"
            "liquid"
            "--bin"
            "electrs"
          ];

          doCheck = false;
        };
      in
      {
        packages = {
          ldk-server = ldkServer;
        };

        devShells.default = pkgs.mkShell {
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          ELEMENTSD_EXEC = "${pkgs.elementsd}/bin/elementsd";
          ELECTRS_LIQUID_EXEC = "${electrsLiquid}/bin/electrs";

          packages =
            (with pkgs; [
              bitcoin
              buf
              cargo
              clippy
              elementsd
              git
              just
              openssl
              pkg-config
              protobuf
              mermaid-cli
              nodejs_20
              rust-analyzer
              rustc
              rustPlatform.rustLibSrc
              rustfmt
              vale
            ])
            ++ [
              electrsLiquid
              ldkServer
              mdx2vast
              textlint
            ];
        };
      }
    );
}
