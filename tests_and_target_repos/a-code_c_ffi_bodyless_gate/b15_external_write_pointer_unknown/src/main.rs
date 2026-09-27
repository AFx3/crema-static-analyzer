use std::ffi::{c_int, c_void};
use std::hint::black_box;

unsafe extern "C" {
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
}

fn main() {
    let b = Box::new([b'Z'; 8]);
    unsafe {
        black_box(write(1, b.as_ptr() as *const c_void, 1));
    }
    drop(b);
}
