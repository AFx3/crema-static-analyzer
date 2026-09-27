use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

fn main() {
    let src = Box::into_raw(Box::new([1_u8; 8]));
    let mut dst = Box::new([2_u8; 8]);
    unsafe {
        drop(Box::from_raw(src));
        black_box(memmove(
            dst.as_mut_ptr().cast::<c_void>(),
            src.cast::<u8>().cast::<c_void>(),
            8,
        ));
    }
    drop(dst);
}
