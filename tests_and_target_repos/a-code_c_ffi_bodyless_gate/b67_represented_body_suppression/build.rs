use std::{env, path::PathBuf, process::Command};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("src/evidence.c");
    assert!(Command::new("/usr/bin/clang-14").args(["-c", "-O0"]).arg(&source).arg("-o").arg(out.join("external.o")).status().unwrap().success());
    assert!(Command::new("ar").arg("crs").arg(out.join("libd3_external.a")).arg(out.join("external.o")).status().unwrap().success());
    println!("cargo:rustc-link-search=native={}",out.display());
    println!("cargo:rustc-link-lib=static=d3_external");
    println!("cargo:rerun-if-changed={}",source.display());
}
