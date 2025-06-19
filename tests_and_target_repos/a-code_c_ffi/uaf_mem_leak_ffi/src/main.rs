use std::ffi::c_void;
unsafe extern "C" {fn cast_and_free_pointer(ptr: *mut c_void);}
unsafe extern "C" {fn free_int(ptr: *mut c_void);}
unsafe extern "C" {fn free_string(ptr: *mut c_void);}
unsafe extern "C" {fn free_str(ptr: *mut c_void);}

fn main() {

    let boxed_int = Box::new(455);
    let int_raw: *mut c_void = Box::into_raw(boxed_int) as *mut c_void;
    println!("Integer allocated in Rust: {}", unsafe { *(int_raw as *mut i32) });
    
    unsafe {
        free_int(int_raw);
        // USE-AFTER-FREE: 
        let int_ptr = int_raw as *mut i32;
        println!("Integer ptr derefenced in Rust -> use-after-free: {}", *int_ptr);
    }
    let boxed = Box::new(98); 
    let a = Box::into_raw(boxed);
    /*  
    unsafe{
    println!("Integer allocated in Rust and forgotten: {}", *a);
    }*/
    /* 
    let boxed = Box::new(9); // Alloca sullo heap un intero
    // forget the own: converte il Box in un raw pointer
    let raw: *mut c_void = Box::into_raw(boxed) as *mut c_void;
    unsafe {
        // Passa il puntatore alla funzione FFI che stampa e libera la memoria
        free_str(raw);
        // USE-AFTER-FREE: dereferenzia il puntatore dopo che la memoria è stata liberata
         let int_pt = raw as *mut i32;
         println!("Integer use-after-free: {}", *int_pt);
    }

    let a = alloc_string("Hello, world!"); // Alloca sullo heap una stringa
    println!("Stringa: {}", a);
    let string_r: *mut c_void = Box::into_raw(a) as *mut c_void;

    unsafe {
        // Passa il puntatore alla funzione FFI che stampa e libera la memoria
        free_str(string_r);
        // USE-AFTER-FREE: dereferenzia il puntatore dopo che la memoria è stata liberata
         let pt = string_r as *mut i32;
         println!("Integer use-after-free: {}", *pt);
    }

    let b= alloc_string("sss");
    println!("Stringa: {}", b);
    let b_raw: *mut c_void = Box::into_raw(b) as *mut c_void;
    */
    
}

 fn alloc_string(s: &str) -> Box<String> {
    let a = Box::new(s.to_string());
    a
}

   /* 
    let string_raw: *mut c_void = Box::into_raw(a) as *mut c_void;
    
     unsafe {
        // La funzione FFI libera la memoria associata alla stringa
        free_str(string_raw);
        
        let recovered_box = string_raw as *mut String;
        println!("String use-after-free: {}", *recovered_box);
     }
     */

/* 
fn main() {
    // ALLOCAZIONE DELL'INTERO CON Box
    let boxed_int = Box::new(90); // Alloca sullo heap un intero
    // forget the own: converte il Box in un raw pointer
    let int_raw: *mut c_void = Box::into_raw(boxed_int) as *mut c_void;
    unsafe {
        // Passa il puntatore alla funzione FFI che stampa e libera la memoria
        cast_and_free_pointer(int_raw);
        // USE-AFTER-FREE: dereferenzia il puntatore dopo che la memoria è stata liberata
         let int_ptr = int_raw as *mut i32;
         println!("Integer use-after-free: {}", *int_ptr);
         //box from raw: converte il raw pointer in Box
        //let _z_box = Box::from_raw(int_raw as *mut i32);
        //let recovered_box = Box::from_raw(int_raw as *mut i32);
        //println!("Integer use-after-free: {}", *recovered_box);
    }
  
    let a_int = Box::new(97); // Alloca sullo heap un intero
    // forget the own: converte il Box in un raw pointer
    let a_raw: *mut c_void = Box::into_raw(a_int) as *mut c_void;
    unsafe {
        // Passa il puntatore alla funzione FFI che stampa e libera la memoria
       cast_and_free_pointer(a_raw);
        // USE-AFTER-FREE: dereferenzia il puntatore dopo che la memoria è stata liberata
       let a_ptr = a_raw as *mut i32;
        println!("Integer use-after-free: {}", *a_ptr);
    }
    

    // ALLOCAZIONE DELLA STRINGA CON Box
   // let boxed_string = alloc_string("Hello, world!"); // Alloca sullo heap una stringa
    // Rimuove l'ownership tramite Box::into_raw
   // let string_raw: *mut c_void = Box::into_raw(boxed_string) as *mut c_void;
   // unsafe {
        // La funzione FFI libera la memoria associata alla stringa
        //free_string(string_raw);
        // USE-AFTER-FREE: tenta di utilizzare la stringa dopo la free
        // Convertiamo il puntatore "dimenticato" di nuovo in Box<String> per accedere al contenuto
        //let recovered_box = string_raw as *mut String;
        //println!("String use-after-free: {}", *recovered_box);
    //}

    let a = alloc_string("Hello, world!"); // Alloca sullo heap una stringa
    let string_raw: *mut c_void = Box::into_raw(a) as *mut c_void;
    
    
     unsafe {
        // La funzione FFI libera la memoria associata alla stringa
        free_string(string_raw);
        
        // USE-AFTER-FREE: tenta di utilizzare la stringa dopo la free
        // Convertiamo il puntatore "dimenticato" di nuovo in Box<String> per accedere al contenuto
        let recovered_box = string_raw as *mut String;
        println!("String use-after-free: {}", *recovered_box);
        //drop(Box::from_raw(recovered_box));
        //drop(string_raw);
    }

    let b = Box::new("Hello!".to_string()); // Alloca sullo heap una stringa
    let b_raw: *mut c_void = Box::into_raw(b) as *mut c_void;
    /* 
    unsafe {
        // La funzione FFI libera la memoria associata alla stringa
        free_str(b_raw);
        // USE-AFTER-FREE: tenta di utilizzare la string
        let recovered_b = b_raw as *mut String;
        println!("String use-after-free: {}", *recovered_b);
    
}
        */
}

 fn alloc_string(s: &str) -> Box<String> {
    let a = Box::new(s.to_string());
    a
}
*/

    
