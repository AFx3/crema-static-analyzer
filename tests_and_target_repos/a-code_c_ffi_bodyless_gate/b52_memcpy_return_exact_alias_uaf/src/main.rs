use std::ffi::c_void;
use std::hint::black_box;
unsafe extern "C" { fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void; }
fn main() {
    let base = Box::into_raw(Box::new([b'A', b'B', b'C', 0_u8, 1, 2, 3, 4]));
    let src = Box::new([1_u8; 8]);
    unsafe {
        let returned = memcpy(base.cast(), src.as_ptr().cast(), 8);
        drop(Box::from_raw(base));
            black_box(*(returned as *const u8));
    }
}
