fn main() {
    cc::Build::new().file("src/ffi.c").compile("crema_v6i_ffi");
}
