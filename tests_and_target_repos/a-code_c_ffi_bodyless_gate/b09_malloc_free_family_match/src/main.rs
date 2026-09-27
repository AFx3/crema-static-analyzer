use std::ffi::c_void;

unsafe extern "C" {
    fn gate_seed_malloc(size: usize) -> *mut c_void;
    fn gate_seed_free(ptr: *mut c_void);
}

fn main() {
    unsafe {
        let p = gate_seed_malloc(64);
        if !p.is_null() {
            gate_seed_free(p);
        }
    }
}
