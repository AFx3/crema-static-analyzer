use std::ffi::c_void;

extern "C"{fn free_str(s: *mut c_void);}
// NO MEMORY ERRORS HERE
fn alloc_str(s: &str) -> Box<String> {
    Box::new(s.to_string())
}

fn alloc_int(i: i32) -> Box<i32> {
    Box::new(i)
}
fn free_string_mir(s: Box<String>) {
    let raw = Box::into_raw(s) as *mut c_void;
    unsafe {
        free_str(raw);

    }
}
fn main() {

    let a = alloc_str("Hello, world!");
    free_string_mir(a);
}