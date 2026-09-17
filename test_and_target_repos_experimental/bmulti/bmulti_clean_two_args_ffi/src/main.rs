use std::os::raw::c_int;

extern "C" {
    fn touch_second(first: *mut c_int, second: *mut c_int);
}

fn main() {
    let a: *mut c_int = Box::into_raw(Box::new(10));
    let b: *mut c_int = Box::into_raw(Box::new(20));

    unsafe {
        touch_second(a, b);

        // Both manual obligations are discharged with the Rust allocator.
        drop(Box::from_raw(a));
        drop(Box::from_raw(b));
    }
}
