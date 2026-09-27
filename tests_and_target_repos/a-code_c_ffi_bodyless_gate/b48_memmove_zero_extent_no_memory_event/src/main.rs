use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

fn main() {
    let src = Box::new([1_u8; 8]);
    let mut dst = Box::new([2_u8; 8]);
    unsafe {
        black_box(memmove(
            dst.as_mut_ptr().cast::<c_void>(),
            src.as_ptr().cast::<c_void>(),
            0,
        ));
    }
    drop(src);
    drop(dst);
}
