use std::ffi::{c_char,c_int}; use std::hint::black_box;
unsafe extern "C" { fn strchr(s:*const c_char,c:c_int)->*mut c_char; }
fn main(){ let b=Box::new([b'A',b'B',0]); unsafe { black_box(strchr(b.as_ptr() as *const c_char,b'B' as c_int)); } drop(b); }
