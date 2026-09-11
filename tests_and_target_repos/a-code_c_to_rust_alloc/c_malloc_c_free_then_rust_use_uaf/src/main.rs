use std::os::raw::c_int;
use std::ptr;

extern "C" {
    fn c_alloc_i32(value: c_int) -> *mut c_int;
    fn c_free_i32(p: *mut c_int);
}

fn main() {
    unsafe {
        let p = c_alloc_i32(21);
        if p.is_null() {
            return;
        }

        c_free_i32(p);

        // Deliberate UAF after a free that occurs inside inlined C/LLVM.
        let _after_free = ptr::read(p);
    }
}
