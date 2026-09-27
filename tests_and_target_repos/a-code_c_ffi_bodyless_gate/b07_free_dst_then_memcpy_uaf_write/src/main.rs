use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

fn main() {
    let src = Box::into_raw(Box::new([1u8; 16]));
    let dst = Box::into_raw(Box::new([2u8; 16]));
    unsafe {
        drop(Box::from_raw(dst));
        black_box(memcpy(
            dst.cast::<c_void>(),
            src.cast::<u8>() as *const c_void,
            16,
        ));
        drop(Box::from_raw(src));
    }
}
