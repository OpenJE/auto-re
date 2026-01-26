{
	description = "An automated depth first llm supported reverse engineering tool";

	inputs = {
		nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
	};

	outputs = { self, nixpkgs }: let
		pkgs = nixpkgs.legacyPackages."x86_64-linux";
	in {
		devShells."x86_64-linux".default = let
		in pkgs.mkShell {
			buildInputs = with pkgs; [
				cargo
				rustc
				rustfmt
				clippy
				rust-analyzer
				clang
				libclang
				rustPlatform.bindgenHook
			];

			env.RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
		};
	};
}
