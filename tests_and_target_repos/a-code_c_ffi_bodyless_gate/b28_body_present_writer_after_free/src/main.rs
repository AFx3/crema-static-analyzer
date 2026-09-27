unsafe extern "C" { fn body_present_write_first(p:*mut u8); }
fn main(){ let p=Box::into_raw(Box::new([9u8;8])); unsafe { drop(Box::from_raw(p)); body_present_write_first(p.cast::<u8>()); } }
