unsafe extern "C" { fn strlen(p: *const u8) -> usize; }
fn main() { unsafe { std::hint::black_box(strlen(b"D3\0".as_ptr())); } }
