fn main() {
    println!("cargo:rerun-if-changed=src/ffi.c");

    cc::Build::new()
        .file("src/ffi.c")
        .compile("ffi");
}