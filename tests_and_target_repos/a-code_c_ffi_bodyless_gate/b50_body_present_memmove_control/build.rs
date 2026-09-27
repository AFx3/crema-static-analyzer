use std::{env, path::PathBuf, process::Command};

fn run(mut command: Command, what: &str) {
    let status = command.status().unwrap_or_else(|error| panic!("{what}: {error}"));
    assert!(status.success(), "{what} failed with {status}");
}

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let object = out.join("memmove.o");
    let library = out.join("libd1_body_present_memmove.a");

    let mut cc = Command::new("cc");
    cc.args(["-O0", "-fno-builtin-memmove", "-c", "src/memmove.c", "-o"])
        .arg(&object);
    run(cc, "compile represented memmove body");

    let mut ar = Command::new("ar");
    ar.arg("crus").arg(&library).arg(&object);
    run(ar, "archive represented memmove body");

    println!("cargo:rerun-if-changed=src/memmove.c");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=d1_body_present_memmove");
}
