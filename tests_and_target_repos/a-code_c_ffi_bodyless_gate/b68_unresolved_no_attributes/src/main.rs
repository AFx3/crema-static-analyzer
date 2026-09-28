unsafe extern "C" { fn d3_unknown(p: *mut u8); }
fn main() { let mut x = Box::new(9_u8); unsafe { d3_unknown((&mut *x) as *mut u8); } std::hint::black_box(x); }
