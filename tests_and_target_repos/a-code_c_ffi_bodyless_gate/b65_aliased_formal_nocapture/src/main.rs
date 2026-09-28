unsafe extern "C" { fn d3_observe(first: *mut u8, second: *mut u8); }
fn main() { let mut x = Box::new(7_u8); let p = (&mut *x) as *mut u8;
unsafe { d3_observe(p, p); } std::hint::black_box(x); }
