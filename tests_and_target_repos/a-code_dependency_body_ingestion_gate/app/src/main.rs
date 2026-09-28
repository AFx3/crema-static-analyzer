use dep::{Compute, Worker};
fn main() {
    let x = std::env::args().count() as u32;
    let _ = dep::ordinary(x);
    let _ = dep::inline_body(x);
    let _ = dep::generic_body::<u64>(x as u64);
    let _ = Worker.compute(x);
    let _ = dep::transitive(x);
}
