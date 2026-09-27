use std::ffi::c_char;
use std::hint::black_box;

unsafe extern "C" {
    fn strlen(s: *const c_char) -> usize;
}

fn main() {
    let p = Box::into_raw(Box::new([b'A', 0, 0, 0, 0, 0, 0, 0]));
    unsafe {
        drop(Box::from_raw(p));
        black_box(strlen(p.cast::<u8>() as *const c_char));
    }
}
