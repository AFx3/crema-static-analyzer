use std::ffi::c_void;
use std::ptr::null_mut;

unsafe extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

fn main() {
    // Runtime-dependent branch: across concrete executions either predecessor
    // can reach the join.  Only one predecessor proves p == NULL, so RN1's
    // intersection domain must not upgrade p to MUST-null.
    let choose_null = std::process::id() % 2 == 0;
    unsafe {
        let p = if choose_null { null_mut() } else { malloc(16) };
        let q = realloc(p, 64);
        if !q.is_null() {
            free(q);
        }
    }
}
