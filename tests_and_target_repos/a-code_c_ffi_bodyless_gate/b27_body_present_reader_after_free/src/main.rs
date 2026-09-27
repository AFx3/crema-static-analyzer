use std::hint::black_box;
unsafe extern "C" { fn body_present_read_first(p:*const u8)->u8; }
fn main(){ let p=Box::into_raw(Box::new([9u8;8])); unsafe { drop(Box::from_raw(p)); black_box(body_present_read_first(p.cast::<u8>())); } }
