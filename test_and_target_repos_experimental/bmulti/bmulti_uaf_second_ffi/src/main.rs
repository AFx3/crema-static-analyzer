use std::os::raw::c_int;

extern "C" {
    fn free_second(first: *mut c_int, second: *mut c_int);
}

fn main() {
    let a: *mut c_int = Box::into_raw(Box::new(10));
    let b: *mut c_int = Box::into_raw(Box::new(20));

    unsafe {
        free_second(a, b);

        // Intentional experimental oracle: B has been deallocated by C.
        let observed = *b;
        std::hint::black_box(observed);

        // A is reclaimed correctly so the target does not accidentally
        // introduce a second leak.
        drop(Box::from_raw(a));
    }
}
