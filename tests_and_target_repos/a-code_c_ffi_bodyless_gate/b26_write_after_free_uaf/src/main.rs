use std::ffi::{c_int,c_void}; use std::hint::black_box;
unsafe extern "C" { fn write(fd:c_int,buf:*const c_void,n:usize)->isize; }
fn main(){ let p=Box::into_raw(Box::new([b'Z';8])); unsafe { drop(Box::from_raw(p)); black_box(write(1,p.cast::<u8>() as *const c_void,1)); } }
