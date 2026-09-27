use std::ffi::{c_int,c_void}; use std::hint::black_box;
unsafe extern "C" { fn memcmp(a:*const c_void,b:*const c_void,n:usize)->c_int; }
fn main(){ let a=Box::into_raw(Box::new([1u8;8])); let b=Box::new([2u8;8]); unsafe { drop(Box::from_raw(a)); black_box(memcmp(a.cast::<u8>() as *const c_void,b.as_ptr() as *const c_void,8)); } drop(b); }
