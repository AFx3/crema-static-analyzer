use std::ffi::{c_int,c_void}; use std::hint::black_box;
unsafe extern "C" { fn memcmp(a:*const c_void,b:*const c_void,n:usize)->c_int; }
fn main(){ let a=Box::new([1u8;8]); let b=Box::into_raw(Box::new([2u8;8])); unsafe { drop(Box::from_raw(b)); black_box(memcmp(a.as_ptr() as *const c_void,b.cast::<u8>() as *const c_void,8)); } drop(a); }
