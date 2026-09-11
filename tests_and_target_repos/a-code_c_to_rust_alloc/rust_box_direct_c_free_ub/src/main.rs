use std::ffi::c_void;

extern "C" {
    fn free(p: *mut c_void);
}

fn main() {
    unsafe {
        let owner = Box::new(17_i32);
        let raw = Box::into_raw(owner);

        // Deliberate allocator-family mismatch control: the pointer is a
        // Rust Box/global-allocation handle, but the deallocator is C free.
        free(raw.cast::<c_void>());
    }
}
