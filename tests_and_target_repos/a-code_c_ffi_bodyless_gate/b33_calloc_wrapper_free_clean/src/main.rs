use std::ffi::c_void;
unsafe extern "C" { fn gate_seed_calloc(n:usize,s:usize)->*mut c_void; fn free(p:*mut c_void); }
fn main(){ unsafe { let p=gate_seed_calloc(2,16); if !p.is_null(){free(p)} } }
