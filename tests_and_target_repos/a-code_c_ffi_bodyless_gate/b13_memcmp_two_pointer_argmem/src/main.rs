use std::ffi::{c_int, c_void};
use std::hint::black_box;

unsafe extern "C" {
    fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int;
}

fn main() {
    let a = Box::new([1u8; 8]);
    let b = Box::new([2u8; 8]);
    unsafe {
        black_box(memcmp(
            a.as_ptr() as *const c_void,
            b.as_ptr() as *const c_void,
            8,
        ));
    }
    drop(a);
    drop(b);
}
