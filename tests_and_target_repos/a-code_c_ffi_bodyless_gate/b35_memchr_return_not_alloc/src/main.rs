use std::ffi::{c_int,c_void}; use std::hint::black_box;
unsafe extern "C" { fn memchr(s:*const c_void,c:c_int,n:usize)->*mut c_void; }
fn main(){ let b=Box::new([1u8,2,3,4]); unsafe { black_box(memchr(b.as_ptr() as *const c_void,3,4)); } drop(b); }
