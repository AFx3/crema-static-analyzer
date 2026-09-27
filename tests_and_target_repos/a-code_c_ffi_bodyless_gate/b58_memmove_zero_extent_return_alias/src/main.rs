use std::ffi::c_void;
use std::hint::black_box;
unsafe extern "C" { fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void; }
fn main() {
    let base = Box::into_raw(Box::new([b'A', b'B', b'C', 0_u8, 1, 2, 3, 4]));
    let src = Box::new([1_u8; 8]);
    unsafe {
        let returned = memmove(base.cast(), src.as_ptr().cast(), 0);
        drop(Box::from_raw(returned.cast::<[u8; 8]>()));
    }
}
