use std::ffi::c_void;
unsafe extern "C" { fn realloc(p:*mut c_void,n:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ let p=Box::into_raw(Box::new([0u8;16])); unsafe { let q=realloc(p.cast::<c_void>(),64); if q.is_null(){ drop(Box::from_raw(p)); } else { free(q); } } }
