extern crate libc;
use std::mem;

pub type MfxHdl = *mut libc::c_void;

#[derive(Debug)]
struct Numbers {
    a: i32,
    b: i32,
    c: i32,
}

impl Numbers {
    pub fn new(a: i32, b: i32, c: i32) -> Self {
        Numbers { a, b, c }
    }
}

impl Drop for Numbers {
    fn drop(&mut self) {
        println!("Dropping Numbers");
    }
}

fn main() {
    let numbers = vec![
        Numbers::new(1, 2, 3),
        Numbers::new(4, 5, 6),
        Numbers::new(7, 8, 9),
    ];
    let len = numbers.len();

    let raw = Box::into_raw(numbers.into_boxed_slice()) as MfxHdl;
    let mut raw_ptr: Vec<MfxHdl> = vec![];
    let size = mem::size_of::<Numbers>() as isize;
    for i in 0..len {
        unsafe { raw_ptr.push(raw.offset((i as isize) * size)) };
    }
    let output = Box::into_raw(raw_ptr.into_boxed_slice()) as MfxHdl;

    let draw = unsafe { Vec::from_raw_parts(output as *mut MfxHdl, len, len) };
    for i in 0..len {
        let ptr: *mut Numbers = draw[i as usize] as *mut Numbers;
        let number = unsafe { &*ptr };
        dbg!(number);
    }
    unsafe { Vec::from_raw_parts(draw[0] as *mut Numbers, len, len) };
}
