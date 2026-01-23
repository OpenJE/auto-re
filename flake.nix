{
	description = "A mathematics programming language";

	inputs = {
		nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
	};

	outputs = { self, nixpkgs }: let
		pkgs = nixpkgs.legacyPackages."x86_64-linux";
	in {
		devShells."x86_64-linux".default = let
			llvm = pkgs.llvmPackages.llvm;
			llvmConfig = pkgs.llvmPackages.llvm.dev;
		in pkgs.mkShell {
			buildInputs = with pkgs; [
				cargo rustc rustfmt clippy rust-analyzer
				llvm
				llvmPackages.clang
				llvmPackages.bintools
				libxml2
			];

			LLVM_SYS_210_PREFIX = "${llvm}";
			LLVM_CONFIG_PATH = "${llvmConfig}/bin/llvm-config";
			env.RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
		};
	};
}
