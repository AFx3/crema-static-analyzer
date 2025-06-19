fn main() {

    let b = Box::new(6);
    let ptr = Box::into_raw(b);
 
    unsafe{ 
        drop(Box::from_raw(ptr));
        drop(Box::from_raw(ptr));
    
    }

}