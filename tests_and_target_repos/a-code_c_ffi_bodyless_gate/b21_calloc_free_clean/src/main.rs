use std::ffi::c_void;
unsafe extern "C" { fn calloc(n:usize,s:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ unsafe { let p=calloc(4,8); if !p.is_null(){free(p)} } }
