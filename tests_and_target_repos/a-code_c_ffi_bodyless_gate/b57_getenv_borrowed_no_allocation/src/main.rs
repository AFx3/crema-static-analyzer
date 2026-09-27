use std::ffi::c_char;
use std::hint::black_box;
unsafe extern "C" { fn getenv(name: *const c_char) -> *mut c_char; }
fn main() { unsafe { black_box(getenv(b"PATH\0".as_ptr().cast())); } }
