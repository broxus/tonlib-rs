use std::env;
use std::fs;
use std::path::PathBuf;

use cmake::Config;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR");
    println!("cargo:rerun-if-changed={}/src/tonlib-sys.cpp", manifest_dir);
    println!("cargo:rerun-if-changed={}/src/tonlib-sys.hpp", manifest_dir);

    let dst = Config::new("./")
        .define("TON_USE_ROCKSDB", "OFF")
        .define("TON_USE_ABSEIL", "OFF")
        .define("TON_ONLY_TONLIB", "ON")
        .define("TON_ARCH", "")
        .define("TON_USE_GDB", "OFF")
        .define("TON_USE_STACKTRACE", "OFF")
        .define("TONLIB_FULL_API", "ON")
        .build_target("tonlib-sys-cpp-bundled")
        .build();

    println!("cargo:rustc-link-search=native={}", dst.display());
    println!("cargo:rustc-link-lib=static=tonlib-sys-cpp-bundled");
    println!("cargo:rustc-link-lib=dylib=crypto");

    let out_dir = env::var_os("OUT_DIR").expect("missing OUT_DIR");
    let path = PathBuf::from(out_dir).join("dummy.cc");
    fs::write(&path, "int rust_link_cplusplus;\n").unwrap();
    cc::Build::new().cpp(true).file(&path).compile("link-cplusplus");
}
