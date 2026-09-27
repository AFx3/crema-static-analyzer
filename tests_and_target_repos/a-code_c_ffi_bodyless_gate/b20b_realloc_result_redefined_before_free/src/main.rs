use std::ffi::c_void;
unsafe extern "C" { fn gate_seed_malloc(size: usize) -> *mut c_void; fn realloc(p:*mut c_void,n:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ unsafe { let p=gate_seed_malloc(16); if p.is_null(){return;} let mut q=realloc(p,64); if q.is_null(){free(p)} else { q=p; free(q); } } }
