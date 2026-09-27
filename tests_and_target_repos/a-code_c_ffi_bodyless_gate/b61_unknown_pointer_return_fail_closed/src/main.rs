use std::hint::black_box;
unsafe extern "C" {
    #[link_name = "getenv"]
    fn opaque_ptr_fn(name: *const std::ffi::c_char) -> *mut std::ffi::c_char;
}
fn main() { unsafe { black_box(opaque_ptr_fn(b"PATH\0".as_ptr().cast())); } }
