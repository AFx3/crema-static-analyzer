use std::ffi::c_char;
use std::hint::black_box;

unsafe extern "C" {
    fn strlen(s: *const c_char) -> usize;
}

fn main() {
    let mut b = Box::new([0u8; 8]);
    b[0] = b'X';
    unsafe {
        black_box(strlen(b.as_ptr() as *const c_char));
    }
    drop(b);
}
