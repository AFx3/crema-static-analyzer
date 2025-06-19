use core::alloc;

/* 
fn alloc_int(s: &i32) -> Box<i32> {
    let a = Box::new(*s);
    a
    
}
fn main() {
    unsafe{
    let s = 4;
    let a = alloc_int(&s);

    let str_raw = Box::into_raw(a);
   // drop(str_raw);
    drop(Box::from_raw(str_raw));
    drop(Box::from_raw(str_raw));
}

}
*/



fn main() {

    let b = Box::new(42);

    let ptr = Box::into_raw(b);
 
 
    // closure with df
    let double_free = || unsafe {
     
        drop(Box::from_raw(ptr));

        drop(Box::from_raw(ptr));
    };

    // Call the closure
    double_free();
    

    /* 
    unsafe{
        
    drop(Box::from_raw(ptr));

    drop(Box::from_raw(ptr));
    
    }
    */

}
