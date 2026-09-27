use std::ffi::c_void;
unsafe extern "C" { fn malloc(n:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ unsafe { let p=malloc(32) as *mut u8; if p.is_null(){return;} free(p.cast::<c_void>()); } }
