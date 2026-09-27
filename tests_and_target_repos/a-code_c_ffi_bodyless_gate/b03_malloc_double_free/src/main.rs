use std::ffi::c_void;

unsafe extern "C" {
    fn gate_seed_malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

fn main() {
    unsafe {
        let p = gate_seed_malloc(32);
        if !p.is_null() {
            free(p);
            free(p);
        }
    }
}
