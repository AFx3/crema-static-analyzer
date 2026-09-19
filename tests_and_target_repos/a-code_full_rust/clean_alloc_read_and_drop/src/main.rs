fn main() {
    let a = Box::new(1);
    let b = Box::new(2);

    let c = a;

    drop(c);
    drop(b);
}