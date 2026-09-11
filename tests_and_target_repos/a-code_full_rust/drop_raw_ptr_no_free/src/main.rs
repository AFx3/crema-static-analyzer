fn main() {
    let boxed = Box::new(42_i32);
    let raw = Box::into_raw(boxed);

    // Dropping a raw pointer does NOT free the pointee.
    std::mem::drop(raw);

    unsafe {
        println!("{}", *raw);
    }
}