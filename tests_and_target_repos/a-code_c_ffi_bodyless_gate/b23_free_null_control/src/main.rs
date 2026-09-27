use std::ffi::c_void; use std::ptr::null_mut;
unsafe extern "C" { fn free(p:*mut c_void); }
fn main(){ unsafe { free(null_mut()); } }
