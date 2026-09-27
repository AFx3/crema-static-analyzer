use std::ffi::{c_int,c_void}; use std::hint::black_box;
unsafe extern "C" { fn memset(p:*mut c_void,c:c_int,n:usize)->*mut c_void; }
fn main(){ let mut b=Box::new([0u8;8]); unsafe { let r=memset(b.as_mut_ptr() as *mut c_void,1,8); black_box(r); } drop(b); }
