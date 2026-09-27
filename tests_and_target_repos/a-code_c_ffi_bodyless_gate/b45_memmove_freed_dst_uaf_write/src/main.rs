use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

fn main() {
    let dst = Box::into_raw(Box::new([2_u8; 8]));
    let src = Box::new([1_u8; 8]);
    unsafe {
        drop(Box::from_raw(dst));
        black_box(memmove(
            dst.cast::<u8>().cast::<c_void>(),
            src.as_ptr().cast::<c_void>(),
            8,
        ));
    }
    drop(src);
}
