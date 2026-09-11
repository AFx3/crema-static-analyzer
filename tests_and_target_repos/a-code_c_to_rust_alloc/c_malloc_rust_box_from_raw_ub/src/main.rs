use std::os::raw::c_int;

extern "C" {
    fn c_alloc_i32(value: c_int) -> *mut c_int;
}

fn main() {
    unsafe {
        let p = c_alloc_i32(99);
        if p.is_null() { return; }
        let owned = Box::from_raw(p);
        println!("{}", *owned);
    }
}
