pub fn ordinary(x: u32) -> u32 { x.wrapping_add(17) ^ 0x2468 }
#[inline]
pub fn inline_body(x: u32) -> u32 { x.rotate_left(5) ^ 0x369a }
pub fn generic_body<T: Into<u64>>(x: T) -> u64 { x.into().wrapping_mul(31) ^ 0x1234 }
pub trait Compute { fn compute(&self, x: u32) -> u32; }
pub struct Worker;
impl Compute for Worker { fn compute(&self, x: u32) -> u32 { x ^ 0x5a5a } }
pub fn transitive(x: u32) -> u32 { dep2::leaf(x).wrapping_add(23) }
