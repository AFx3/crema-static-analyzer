
// MEMORY LEAK
fn main() {
    
    
    let p = Box::new("astro");
    std::mem::forget(p);
    println!("a");
}
// ok int, float