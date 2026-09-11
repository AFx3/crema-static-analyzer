use std::os::raw::c_int;

extern "C" {
    fn c_alloc_four_i32() -> *mut c_int;
}

fn main() {
    unsafe {
        let p = c_alloc_four_i32();
        if p.is_null() { return; }
        let v = Vec::from_raw_parts(p, 4, 4);
        println!("{:?}", v);
    }
}
