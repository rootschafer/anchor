{
  description = "Anchor - Rust implementation of the klipper protocol";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        isLinux = pkgs.stdenv.isLinux;

        klipperDeps = [
          pkgs.python313Packages.python
          pkgs.python313Packages.greenlet
          pkgs.python313Packages.cffi
          pkgs.python313Packages.pyserial
          pkgs.jinja2-cli
        ];

        moonrakerDeps = [
          pkgs.python313Packages.importlib-metadata
          pkgs.python313Packages.tornado
          pkgs.python313Packages.streaming-form-data
          pkgs.python313Packages.dbus-fast
          pkgs.python313Packages.inotify-simple
          pkgs.python313Packages.libnacl
          pkgs.python313Packages.distro
          pkgs.python313Packages.virtualenv
        ];

        commonNativeBuildInputs = [
          pkgs.flip-link
          pkgs.python313Packages.pyserial
        ] ++ (
          if isLinux then
            [
              pkgs.systemd.dev
            ]
          else
            [ ]
        );
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
          ] ++ klipperDeps ++ moonrakerDeps;

          nativeBuildInputs = commonNativeBuildInputs;

          shellHook = ''
            echo "Anchor development environment"
            echo "Rust: $(rustc --version)"
            echo "Cargo: $(cargo --version)"
          '';
        };
      });
}

