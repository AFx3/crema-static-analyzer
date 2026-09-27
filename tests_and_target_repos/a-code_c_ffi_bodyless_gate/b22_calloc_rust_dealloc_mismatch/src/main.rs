use std::alloc::{dealloc,Layout}; use std::ffi::c_void;
unsafe extern "C" { fn calloc(n:usize,s:usize)->*mut c_void; }
fn main(){ unsafe { let p=calloc(4,8) as *mut u8; if p.is_null(){return;} dealloc(p,Layout::from_size_align(32,8).unwrap()); } }
