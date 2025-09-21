{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    inputs:
    inputs.flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import inputs.nixpkgs { inherit system; };

        rustToolchain = with pkgs; [
          cargo
          clippy
          rust-analyzer
          rustc
          rustfmt
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          packages = rustToolchain;
        };

        # FIXME: Add tests
        checks =
          let
            check =
              {
                name,
                command,
                packages,
              }:
              {
                ${name} = pkgs.stdenv.mkDerivation {
                  inherit name;
                  src = inputs.self;

                  nativeBuildInputs = packages;

                  buildPhase = ''
                    ${command}
                    touch $out
                  '';
                };
              };
          in
          check {
            name = "typos";
            command = "typos";
            packages = [ pkgs.typos ];
          }
          // check {
            name = "format";
            command = "cargo fmt --check";
            packages = rustToolchain;
          }
          // check {
            name = "lint";
            command = "cargo clippy -- -Dwarnings";
            packages = rustToolchain;
          };
      }
    );
}
