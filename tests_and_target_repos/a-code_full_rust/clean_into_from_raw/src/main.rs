fn main(){

  let  b1 = Box::new(99);
  println!("boxed value = {}", *b1);
        
  // forget the ownership
  let raw = Box::into_raw(b1);
  println!("forgetting the ownership via into_raw");

 unsafe {
  *raw += 1; // 
  println!("dereference the raw pointer and increasing the value by 1");
  println!("raw value = {}", *raw);
  }

  // NO ERROR: take back the ownership, free insruction for the heap memory at the end of the scope
  unsafe { let _ = Box::from_raw(raw); };

}