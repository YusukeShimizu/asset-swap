{
  description = "Rust template repository (dev env via Nix Flakes)";

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
      in
      {
        devShells.default = pkgs.mkShell {
          packages =
            (with pkgs; [
              buf
              cargo
              clippy
              git
              just
              mermaid-cli
              nodejs_20
              rust-analyzer
              rustc
              rustfmt
              vale
            ])
            ++ [
              mdx2vast
              textlint
            ];
        };
      }
    );
}
