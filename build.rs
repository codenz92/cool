use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_SYS_170_PREFIX");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let Some(prefix) = env::var_os("LLVM_SYS_170_PREFIX") else {
        return;
    };

    let prefix = PathBuf::from(prefix);
    println!("cargo:rustc-link-search=native={}", prefix.join("lib").display());
    println!("cargo:rustc-link-lib=dylib=LLVM-C");

    cc::Build::new()
        .file("vendor/llvm/target_wrappers.c")
        .include(prefix.join("include"))
        .compile("cool_llvm_target_wrappers");
}
