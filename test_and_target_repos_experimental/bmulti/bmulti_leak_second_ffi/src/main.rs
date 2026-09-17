use std::os::raw::c_int;

extern "C" {
    fn free_first_only(first: *mut c_int, second: *mut c_int);
}

fn main() {
    let a: *mut c_int = Box::into_raw(Box::new(10));
    let b: *mut c_int = Box::into_raw(Box::new(20));

    unsafe {
        free_first_only(a, b);
    }

    // EXPECT:
    // a: deallocated by C free -> allocator mismatch candidate.
    // b: ownership remains manual and unreclaimed -> leak candidate.
}