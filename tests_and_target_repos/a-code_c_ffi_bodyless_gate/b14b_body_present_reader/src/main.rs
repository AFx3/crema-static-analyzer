use std::hint::black_box;

unsafe extern "C" {
    fn body_present_read_first(p: *const u8) -> u8;
}

fn main() {
    let b = Box::new([9u8; 8]);
    unsafe { black_box(body_present_read_first(b.as_ptr())); }
    drop(b);
}
