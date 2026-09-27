use std::ffi::c_void;

unsafe extern "C" {
    fn gate_seed_malloc(size: usize) -> *mut c_void;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

fn main() {
    unsafe {
        let p = gate_seed_malloc(16);
        if p.is_null() {
            return;
        }
        let q = realloc(p, 64);
        if q.is_null() {
            free(p);
        } else {
            free(p);
            free(q);
        }
    }
}
