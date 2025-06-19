use std::ptr;

struct Point {
    x: i8,
    y: i8,
}

struct Rectangle {
    top_left: Point,
    bottom_right: Point,
}

fn build_boxed_point(x: i8, y: i8) -> Box<Point> {
    Box::new(Point { x, y })
}

fn build_rectangle(top_left: Point, bottom_right: Point) -> Rectangle {
    Rectangle { top_left, bottom_right }
}

fn main() {
    // 1) Allocate two boxed points:
    let boxed1 = build_boxed_point(0, 5);
    let boxed2 = build_boxed_point(0, 10);

    // 2) Turn each Box<Point> into a raw pointer:
    let raw1: *mut Point = Box::into_raw(boxed1);
    let raw2: *mut Point = Box::into_raw(boxed2);
    // std::boxed::Box::<Point>::into_raw

    // 3) Read the Point value out of the heap (moves it into p1/p2),
    //    but does *not* deallocate the heap memory:
    let p1: Point = unsafe { ptr::read(raw1) };
    let p2: Point = unsafe { ptr::read(raw2) };

    // MEMORY LEAK:
    //  - p1 and p2 are stack‐local copies of the two Points
    //  - raw1/raw2 still point to valid heap memory, but never free 

    // build rectangle
    let rect = build_rectangle(p1, p2);
    println!("Top left: ({}, {})", rect.top_left.x, rect.top_left.y);
    println!("Bottom right: ({}, {})", rect.bottom_right.x, rect.bottom_right.y);

    // REMOVE MEMORY LEAK
    // get back the ownership of the heap memory with from raw
    let _ = unsafe { Box::from_raw(raw1) };
    let _ = unsafe { Box::from_raw(raw2) };

   
}
