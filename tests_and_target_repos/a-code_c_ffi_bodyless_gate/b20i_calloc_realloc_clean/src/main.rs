use std::ffi::c_void;
unsafe extern "C" { fn calloc(n:usize,s:usize)->*mut c_void; fn realloc(p:*mut c_void,n:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ unsafe { let p=calloc(2,8); if p.is_null(){return;} let q=realloc(p,64); if q.is_null(){free(p)} else {free(q)} } }
