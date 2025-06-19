use std::cell::{RefCell, UnsafeCell};
use std::ops::Deref;
use std::rc::Rc as StdRc;
use std::time::{Duration, Instant};

// ========================
// 1. Define the Entity
// ========================

#[derive(Debug)]
struct Entity {
    id: usize,
    x: f32,
    y: f32,
}

impl Entity {
    fn update(&mut self) {
        // Simple update: move the entity
        self.x += 1.0;
        self.y += 1.0;
    }
}

// ========================
// 2. Define the RcLike Trait
// ========================

/// A trait that encapsulates the behaviors of a reference-counted smart pointer.
/// It requires implementing `Clone` and `Deref` (and optionally `DerefMut`).
trait RcLike<T>: Clone + Deref<Target = T> + Constructor<T> {}
trait Constructor<T> {
    fn new(value: T) -> Self;
}

// ========================
// 3. Implement RcLike for StdRc
// ========================

struct StdRcWrapper<T>(StdRc<T>);

impl<T> Clone for StdRcWrapper<T> {
    fn clone(&self) -> Self {
        StdRcWrapper(StdRc::clone(&self.0))
    }
}

impl<T> Deref for StdRcWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Constructor<T> for StdRcWrapper<T> {
    fn new(value: T) -> Self {
        StdRcWrapper(StdRc::new(value))
    }
}

// ========================
// 4. Implement a Simple CustomRc
// ========================

use std::ptr::NonNull;

/// A simplified CustomRc implementation for benchmarking.
/// Note: This implementation is not thread-safe and is for benchmarking purposes only.

struct CustomRcInner<T> {
    ref_count: UnsafeCell<usize>,
    value: T,
}

pub struct CustomRc<T> {
    ptr: NonNull<CustomRcInner<T>>,
}

impl<T> CustomRc<T> {
    /// Creates a new CustomRc instance.
    pub fn new(value: T) -> Self {
        let boxed = Box::new(CustomRcInner {
            ref_count: UnsafeCell::new(1),
            value,
        });
        CustomRc {
            ptr: unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) },
        }
    }

    /// Decrements the reference count and deallocates if it reaches zero.
    fn drop_rc(&mut self) {
        let inner = unsafe { self.ptr.as_mut() };
        let count = unsafe { &mut *inner.ref_count.get() };
        *count -= 1;
        if *count == 0 {
            unsafe {
                let _ = Box::from_raw(self.ptr.as_ptr());
            }
        }
    }
}

impl<T> Clone for CustomRc<T> {
    fn clone(&self) -> Self {
        unsafe {
            let inner = self.ptr.as_ref();
            let old_count = *inner.ref_count.get();
            // We know this is safe as long as we're single-threaded
            let inner = &mut *self.ptr.as_ptr();
            let count = &mut *inner.ref_count.get();
            *count = old_count + 1;
        }
        CustomRc { ptr: self.ptr }
    }
}

impl<T> Deref for CustomRc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &self.ptr.as_ref().value }
    }
}

impl<T> Drop for CustomRc<T> {
    fn drop(&mut self) {
        self.drop_rc();
    }
}

impl Constructor<Entity> for CustomRc<Entity> {
    fn new(value: Entity) -> Self {
        CustomRc::new(value)
    }
}

// ========================
// 5. Define the Game Structure
// ========================

struct Game<RcType>
where
    RcType: RcLike<Entity>,
{
    entities: Vec<RcType>,
    frames: usize,
    operations_per_frame: usize,
}

impl<RcType> Game<RcType>
where
    RcType: RcLike<Entity>,
{
    fn new(frames: usize, operations_per_frame: usize) -> Self {
        Game {
            entities: Vec::new(),
            frames,
            operations_per_frame,
        }
    }

    fn setup(&mut self, num_entities: usize) {
        for id in 0..num_entities {
            let entity = Entity { id, x: 0.0, y: 0.0 };
            self.entities.push(RcType::clone(&RcType::new(entity)));
        }
    }

    fn run(&mut self) {
        for frame in 0..self.frames {
            for _ in 0..self.operations_per_frame {
                // Iterate through entities and perform operations
                for entity_rc in &self.entities {
                    let cloned_rc = entity_rc.clone();
                    let entity = cloned_rc.deref();
                    // Perform some dummy calculations
                    let _ = std::hint::black_box(entity.x + entity.y);
                    // cloned_rc goes out of scope here
                }
            }
            // Optionally, print progress
            if frame % (self.frames / 10).max(1) == 0 {
                println!("Completed frame {}/{}", frame, self.frames);
            }
        }
    }
}

impl<RcType> RcLike<Entity> for RcType where
    RcType: Clone + Deref<Target = Entity> + Constructor<Entity>
{
}

// ========================
// 6. Benchmarking Function
// ========================

fn benchmark<RcType>(name: &str, frames: usize, operations_per_frame: usize, num_entities: usize)
where
    RcType: RcLike<Entity>,
{
    println!("Benchmarking {}...", name);
    let start = Instant::now();
    let mut game = Game::<RcType>::new(frames, operations_per_frame);
    game.setup(num_entities);
    game.run();
    let duration = start.elapsed();
    println!(
        "{} completed in {:?} ({} frames, {} operations/frame)\n",
        name, duration, frames, operations_per_frame
    );
}

// ========================
// 7. Main Function
// ========================

fn main() {
    // Configuration
    let num_entities = 25_000; // Number of entities in the game
    let num_frames = 25; // Number of frames to simulate
    let operations_per_frame = 10_000; // Number of operations per frame

    // Warm-up (optional)
    println!("Warming up...");
    {
        let mut game = Game::<StdRcWrapper<Entity>>::new(num_frames, operations_per_frame);
        game.setup(num_entities);
        game.run();
    }
    println!("Warm-up completed.\n");

    // Benchmark StdRc
    benchmark::<StdRcWrapper<Entity>>("StdRc", num_frames, operations_per_frame, num_entities);

    // Benchmark CustomRc
    benchmark::<CustomRc<Entity>>("CustomRc", num_frames, operations_per_frame, num_entities);
}
