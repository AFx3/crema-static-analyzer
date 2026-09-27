use std::ffi::c_char; use std::hint::black_box;
unsafe extern "C" { fn strdup(s:*const c_char)->*mut c_char; }
fn main(){ let s=b"abc\0"; unsafe { black_box(strdup(s.as_ptr() as *const c_char)); } }
