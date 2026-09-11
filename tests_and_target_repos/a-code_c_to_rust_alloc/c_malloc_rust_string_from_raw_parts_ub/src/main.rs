extern "C" {
    fn c_alloc_hello() -> *mut u8;
}

fn main() {
    unsafe {
        let p = c_alloc_hello();
        if p.is_null() {
            return;
        }

        // The analyzer should report the ownership/allocator contract as not
        // established from positive C malloc-family provenance.  The target is
        // compiled and analyzed; it is not executed by the Phase-5 harness.
        let s = String::from_raw_parts(p, 5, 6);
        println!("{}", s);
    }
}
