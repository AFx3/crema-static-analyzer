use std::ffi::c_void;

unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

fn main() {
    let p = Box::into_raw(Box::new([7u8; 32]));
    unsafe {
        free(p.cast::<c_void>());
    }
}
