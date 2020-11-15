use cmake::Config;

fn main() {
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

    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-search=native={}", dst.display());
    println!("cargo:rustc-link-lib=static=tonlib-sys-cpp-bundled");
}
