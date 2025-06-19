#[derive(Copy, Clone)]
struct Point {
    x: i8,
    y: i8,
}

struct Rectangle {
    top_left: Point,
    bottom_right: Point,
}

fn build_boxed_point(x:i8,y:i8) -> Box<Point> {
    // allocate this point on the heap, and return a pointer to it
    Box::new(Point { x, y, })
}

fn build_rectangle(top_left:Point, bottom_right:Point) -> Rectangle {
    Rectangle {
        top_left,
        bottom_right,
    }
}

//MEMORY LEAK
fn main() {
    // allocate two boxed points
    let boxed1 = build_boxed_point(0, 5);
    let boxed2 = build_boxed_point(0, 10);

 
    // move the Point out of each Box
    let p1: Point = *boxed1;
    let p2: Point = *boxed2;
   // LEAK: forget the first box so its heap memory is never freed
   std::mem::forget(boxed1);


    // build a Rectangle from the stack-copied Points
    let rect = build_rectangle(p1, p2);

    println!("Top left: ({}, {})", rect.top_left.x, rect.top_left.y);
    println!("Bottom right: ({}, {})", rect.bottom_right.x, rect.bottom_right.y);
}
