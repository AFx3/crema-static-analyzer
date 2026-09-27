use std::ffi::c_void;
use std::hint::black_box;
unsafe extern "C" { fn gate_seed_malloc(size: usize) -> *mut c_void; fn realloc(p:*mut c_void,n:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ unsafe { let p=gate_seed_malloc(16); if p.is_null(){return;} let q=realloc(p,64); black_box(0usize); if q.is_null(){free(p)} else {free(q)} } }
