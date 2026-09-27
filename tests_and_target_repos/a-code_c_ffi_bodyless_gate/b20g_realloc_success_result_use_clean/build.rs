use std::{env, path::PathBuf, process::Command};
fn run(mut cmd: Command, what: &str) {
    let status = cmd.status().unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(status.success(), "{what} failed with {status}");
}
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let obj = out.join("ffi.o");
    let lib = out.join("libffi_matrix_seed.a");
    let mut cc = Command::new("cc");
    cc.args(["-c", "src/ffi.c", "-o"]).arg(&obj); run(cc, "cc");
    let mut ar = Command::new("ar");
    ar.arg("crus").arg(&lib).arg(&obj); run(ar, "ar");
    println!("cargo:rerun-if-changed=src/ffi.c");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ffi_matrix_seed");
}
