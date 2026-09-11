use std::os::raw::c_ulonglong;

extern "C" {
    fn c_calloc_u64() -> *mut c_ulonglong;
}

fn main() {
    unsafe {
        let p = c_calloc_u64();
        if p.is_null() { return; }
        println!("{}", *p);
        // Deliberate leak.
    }
}
