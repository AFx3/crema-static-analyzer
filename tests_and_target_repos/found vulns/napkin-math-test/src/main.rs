extern crate libc;

use std::time::Duration;
use std::thread;


struct BenchmarkResult {
    iterations: usize,
    duration: Duration,
    cycles: u64,
}

impl BenchmarkResult {
    fn print_results(&self, name: &str, size_of_type: usize) {
        println!("[{}] Iterations: {}", name, self.iterations);
        println!("[{}] Duration (ms): {}", name, self.duration.as_millis());
    }
}

fn benchmark<T, F: Fn() -> T, V: FnMut(&mut T) -> bool>(setup: F, mut f: V) -> Result<BenchmarkResult, ()> {
// EXECUTE ONLY ONE ITERATION FOR SIMPLICITY
    let mut val = setup();
    let start = std::time::Instant::now();
    let mut count = 0;
    while f(&mut val) {
        count += 1;
        if count >= 1 {
            break;
        }
    }
    let duration = start.elapsed();
    Ok(BenchmarkResult { iterations: count, duration, cycles: 0 })
}



fn syscall_getrusage() {
    let time = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let rusage = Box::new(libc::rusage {
        ru_utime: time,
        ru_stime: time,
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    });
    let ptr = Box::into_raw(rusage);

    let result = benchmark(
        || {},
        |_test| {
            unsafe {
                libc::getrusage(0, ptr);
            }
            true
        },
    )
    .unwrap();
    result.print_results("Sycall getrusage(2)", 0);

    println!("Leaked rusage at: {:p}", ptr);
}

fn main() {
   
    syscall_getrusage();
    thread::sleep(Duration::from_secs(1));
}




// src/main.rs
/* 
use std::time::{Duration, Instant};
use std::{env, ptr, thread, mem};
use std::io::Write as FmtWrite;
use byte_unit::Byte;
use num_format::{Locale, ToFormattedString};
use libc;
use std::fmt::Write;
use std::str::FromStr;
use byte_unit::n_gib_bytes;
use byte_unit::n_mib_bytes;
use byte_unit::n_tib_bytes;

/// Risultato del benchmark
pub struct BenchmarkResult {
    pub iterations: usize,
    pub duration: Duration,
    pub cycles: u64,
}

impl BenchmarkResult {
    pub fn print_results(&self, name: &str, size_of_type: usize) {
        let mut name = String::from(name);
        if size_of_type > 0 {
            write!(
                name,
                " <{}>",
                Byte::from_bytes(size_of_type as u128)
                    .get_appropriate_unit(true)
                    .format(0)
            )
            .unwrap();
        }

        println!(
            "\n[{}] Iterations in {} milliseconds, no overhead: {}",
            name,
            self.duration.as_millis(),
            self.iterations.to_formatted_string(&Locale::en)
        );

        println!(
            "[{}] Iterations / second: {}",
            name,
            ((self.iterations as f64 / self.duration.as_millis() as f64) * 1000.0) as u128
        );

        if size_of_type > 0 {
            println!("[{}] Bytes per iteration: {}", name, size_of_type);
            let total_bytes = size_of_type * self.iterations;
            println!(
                "[{}] Total bytes processed: {}",
                name,
                Byte::from_bytes(total_bytes as u128)
                    .get_appropriate_unit(true)
                    .format(3)
            );
            let bytes_per_sec = (total_bytes as f64 / self.duration.as_millis() as f64) * 1000.0;
            println!(
                "[{}] Throughput: {}/s",
                name,
                //Byte::from_bytes(bytes_per_sec as u128) OLD VERSOIN
                Byte::from_bytes(bytes_per_sec as u128)
                    .get_appropriate_unit(true)
                    .format(3)
            );
        }

        let nanos = self.duration.as_nanos() as u64 / self.iterations as u64;
        let single_op = Duration::from_nanos(nanos);
        let time_unit = if single_op.as_nanos() <= 10 {
            format!("{:.3} ns", self.duration.as_nanos() as f64 / self.iterations as f64)
        } else {
            self.get_appropriate_time_unit(single_op)
        };
        println!("[{}] Avg single iteration: {}", name, time_unit);

        let cycles_per_iter = self.cycles as f64 / self.iterations as f64;
        println!("[{}] Avg single iteration cycles: {:.2}", name, cycles_per_iter);

        if size_of_type > 0 {
            let nanos_per_byte = 1.0 / ((size_of_type as f64) / self.duration.as_nanos() as f64);
            let mib_time = Duration::from_nanos((nanos_per_byte * n_mib_bytes!(1) as f64) as u64);
            println!(
                "[{}] Time to process 1 MiB: {}",
                name,
                self.get_appropriate_time_unit(mib_time)
            );
            let gib_time = Duration::from_nanos((nanos_per_byte * n_gib_bytes!(1) as f64) as u64);
            println!(
                "[{}] Time to process 1 GiB: {}",
                name,
                self.get_appropriate_time_unit(gib_time)
            );
            let tib_time = Duration::from_nanos((nanos_per_byte * n_tib_bytes!(1) as f64) as u64);
            println!(
                "[{}] Time to process 1 TiB: {}",
                name,
                self.get_appropriate_time_unit(tib_time)
            );
        }
    }

    fn get_appropriate_time_unit(&self, duration: Duration) -> String {
        if duration.as_nanos() < 1_000 {
            format!("{} ns", duration.as_nanos())
        } else if duration.as_nanos() < 1_000_000 {
            format!("{} μs", duration.as_micros())
        } else if duration.as_micros() < 3_000_000 {
            format!("{} ms", duration.as_millis())
        } else if duration.as_secs() <= 120 {
            format!("{:.2} s", duration.as_secs_f64())
        } else if duration.as_secs() <= 3600 {
            format!("{:.2} min", duration.as_secs_f64() / 60.0)
        } else {
            format!("{:.2} h", duration.as_secs_f64() / 3600.0)
        }
    }
}

/// Esegue il benchmark
pub fn benchmark<T, F: Fn() -> T, V: FnMut(&mut T) -> bool>(
    setup: F,
    mut f: V,
) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
    // Warmup
    let mut val = setup();
    let warmup = Duration::from_millis(100);
    let mut iter_per_check = 1;
    let mut iters = 0;
    let start = Instant::now();
    let mut done = false;
    while start.elapsed() < warmup {
        for i in 0..iter_per_check {
            if !f(&mut val) {
                done = true;
                iter_per_check = i + 1;
                break;
            }
        }
        iters += iter_per_check;
        if done { break; }
    }

    // Real run
    thread::sleep(Duration::from_secs(1));
    let mut val = setup();
    let run_dur = Duration::from_millis(5000);
    let mut iter_per_check = iters.max(1);
    let mut iters = 0;
    let start = Instant::now();
    let mut done = false;
    while start.elapsed() < run_dur {
        for i in 0..iter_per_check {
            if !f(&mut val) {
                done = true;
                iter_per_check = i + 1;
                break;
            }
        }
        iters += iter_per_check;
        if done { break; }
    }
    let actual = start.elapsed();

    Ok(BenchmarkResult {
        iterations: iters,
        duration: actual,
        cycles: 0,
    })
}

fn syscall_getrusage() {
    // Prepara una struct rusage su heap e ne perde il Box
    let time = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let rusage = Box::new(libc::rusage {
        ru_utime: time,
        ru_stime: time,
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    });
    let ptr = Box::into_raw(rusage);

    let result = benchmark(
        || {},
        |_test| {
            unsafe {
                libc::getrusage(libc::RUSAGE_SELF, ptr);
            }
            true
        },
    )
    .unwrap();

    result.print_results("Syscall getrusage(2)", 0);


}

fn main() {
    println!("Running getrusage benchmark (this will leak one rusage struct)...");
    syscall_getrusage();
    println!("Done. Sleeping to allow leak inspection...");
    thread::sleep(Duration::from_secs(2));
}*/


/* MINIMAL
extern crate libc;

use std::ptr;

fn syscall_getrusage() {

    let time = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
  
    let rusage = Box::new(libc::rusage {
        ru_utime: time,
        ru_stime: time,
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    });
  
    let ptr: *mut libc::rusage = Box::into_raw(rusage);


    unsafe {
        libc::getrusage(libc::RUSAGE_SELF, ptr);
    }
    println!("Done with getrusage; leaked one rusage struct at {:p}", ptr);
}

fn main() {
    syscall_getrusage();

    // Rimani in vita un secondo per poter ispezionare il leak esternamente
    std::thread::sleep(std::time::Duration::from_secs(1));
}

*/

/* okk
extern crate libc;

use std::time::Duration;
use std::thread;


struct BenchmarkResult {
    iterations: usize,
    duration: Duration,
    cycles: u64,
}

impl BenchmarkResult {
    fn print_results(&self, name: &str, size_of_type: usize) {
        println!("[{}] Iterations: {}", name, self.iterations);
        println!("[{}] Duration (ms): {}", name, self.duration.as_millis());
    }
}

fn benchmark<T, F: Fn() -> T, V: FnMut(&mut T) -> bool>(
    setup: F,
    mut f: V,
) -> Result<BenchmarkResult, ()> {
    // versione semplificata: eseguo solo 1 iterazione per chiarezza
    let mut val = setup();
    let start = std::time::Instant::now();
    let mut count = 0;
    while f(&mut val) {
        count += 1;
        if count >= 1 {
            break;
        }
    }
    let duration = start.elapsed();
    Ok(BenchmarkResult { iterations: count, duration, cycles: 0 })
}



fn syscall_getrusage() {
    let time = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let rusage = Box::new(libc::rusage {
        ru_utime: time,
        ru_stime: time,
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    });
    let ptr = Box::into_raw(rusage);

    let result = benchmark(
        || {},
        |_test| {
            unsafe {
                libc::getrusage(0 /* RUSAGE_SELF */, ptr);
            }
            true
        },
    )
    .unwrap();
    result.print_results("Sycall getrusage(2)", 0);

    // Nota: non chiamiamo mai Box::from_raw(ptr), quindi la memoria resta "leaked"
    println!("Leaked rusage at: {:p}", ptr);
}

fn main() {
    // Esegui solo la syscall getrusage
    syscall_getrusage();

    // Mantieni vivo il processo per ispezionare il leak
    thread::sleep(Duration::from_secs(1));
}
*/



/* 
#[macro_use]
extern crate byte_unit;
extern crate clap;
extern crate regex;
use std::fmt::Write as FmtWrite;

// use std::alloc::System;
// #[global_allocator]
// static A: System = System;

extern crate libc;

// #[cfg(target_os = "linux")]
// use libc::posix_fadvise;

use regex::Regex;
use std::process::Command;

// #[cfg(target_os = "linux")]
// use rio::{Rio, Uring};

#[cfg(target_os = "linux")]
use std::os::unix::io::*;

extern crate jemallocator;
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

static FILE_NAME: &str = "/tmp/napkin.txt";

// https://ark.intel.com/content/www/us/en/ark/products/97185/intel-core-i7-7700hq-processor-6m-cache-up-to-3-80-ghz.html
// https://en.wikichip.org/wiki/intel/core_i7/i7-7700hq
//
// Single: 17.88 GiB/s
// Dual:   37.5 GB/s
//
// L1: 32 KiB
// L2: 262 KiB
// L3: 6 MiB
//
// sysctl -a | grep cache <---
//
// TODO: Would be cool to instrument branch misses etc. here
use byte_unit::Byte;
use clap::{Command as App, Arg};
// use failure::Error;
use mysql::prelude::*;
use mysql::{params, Opts, Pool, Result};
use num_format::{Locale, ToFormattedString};
use rand::seq::SliceRandom;
use rand::thread_rng;
use redis::Commands;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind;
use std::io::SeekFrom;
use std::mem::forget;
use std::net::{TcpListener, TcpStream};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

#[allow(unused_imports)]
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::__rdtscp;
// #[cfg(target_arch = "x86_64")]
// use std::arch::x86_64::*;

// TODO: use this instead
// from bencher::black_box, avoid compiler dead-code optimizations.
pub fn black_box<T>(dummy: T) -> T {
    unsafe {
        let ret = ptr::read_volatile(&dummy);
        forget(dummy);
        ret
    }
}

// TODO: Probably we should just expose duration and iterations, and correct for duration_ratio
// directly in whatever produces this data structure to simplify consumption.
struct BenchmarkResult {
    iterations: usize,
    duration: Duration,
    // duration_ratio: f64,
    // intended_duration: Duration,
    cycles: u64,
}

impl BenchmarkResult {
    fn print_results(&self, name: &str, size_of_type: usize) {
        let mut name = String::from(name);
        if size_of_type > 0 {
            write!(name,
                " <{}>",
                Byte::from_bytes(size_of_type as u128)
                    .get_appropriate_unit(true)
                    .format(0)
            ).unwrap();
        }

        println!(
            "\n[{}] Iterations in {} miliseconds, no overhead: {}",
            name,
            self.duration.as_millis(),
            self.iterations.to_formatted_string(&Locale::en)
        );

        println!(
            "[{}] Iterations / second: {}",
            name,
            ((((self.iterations as f64 / self.duration.as_millis() as f64) as f64) * 1000.0)
                as u128)
                .to_formatted_string(&Locale::en)
        );

        if size_of_type > 0 {
            println!(
                "[{}] Bytes handled per iteration: {} bytes",
                name, size_of_type
            );

            let total_bytes_pushed = size_of_type * self.iterations;
            println!(
                "[{}] Total bytes processed: {}",
                name,
                Byte::from_bytes(total_bytes_pushed as u128)
                    .get_appropriate_unit(true)
                    .format(3)
            );

            let bytes_per_second =
                ((total_bytes_pushed as f64) / self.duration.as_millis() as f64) * 1000.0;
            println!(
                "[{}] Throughput: {}/s",
                name,
                // TODO: Too hard to get right when values aren't just printed!
                Byte::from_bytes(bytes_per_second as u128)
                    .get_appropriate_unit(true)
                    .format(3)
            );
        }

        // TODO handle less than 1ns
        let single_operation_nanoseconds =
            Duration::from_nanos(self.duration.as_nanos() as u64 / self.iterations as u64);

        let time_unit = if single_operation_nanoseconds.as_nanos() <= 10 {
            format!(
                "{:.3} ns",
                self.duration.as_nanos() as f64 / self.iterations as f64
            )
        } else {
            self.get_appropriate_time_unit(single_operation_nanoseconds)
        };

        println!("[{}] Avg single iteration: {}", name, time_unit);

        let single_operation_cycles = self.cycles as f64 / (self.iterations as f64);

        println!(
            "[{}] Avg single iteration cycles: {:.2}",
            name, single_operation_cycles,
        );

        if size_of_type > 0 {
            let single_op_nanos = self.duration.as_nanos() as f64 / self.iterations as f64;
            let nanoseconds_per_byte = 1.0 / ((size_of_type as f64) / single_op_nanos);
            let nanoseconds_per_mebibyte = nanoseconds_per_byte * n_mib_bytes!(1) as f64;
            let duration_per_mebibyte = Duration::from_nanos(nanoseconds_per_mebibyte as u64);

            println!(
                "[{}] Time to process 1 MiB: {}",
                name,
                self.get_appropriate_time_unit(duration_per_mebibyte),
            );

            let nanoseconds_per_gibibyte = nanoseconds_per_byte * n_gib_bytes!(1) as f64;
            let duration_per_gibibyte = Duration::from_nanos(nanoseconds_per_gibibyte as u64);

            println!(
                "[{}] Time to process 1 GiB: {}",
                name,
                self.get_appropriate_time_unit(duration_per_gibibyte),
            );

            let nanoseconds_per_tibibyte = nanoseconds_per_byte * n_tib_bytes!(1) as f64;
            let duration_per_tibibyte = Duration::from_nanos(nanoseconds_per_tibibyte as u64);

            println!(
                "[{}] Time to process 1 TiB: {}",
                name,
                self.get_appropriate_time_unit(duration_per_tibibyte),
            );
        }
    }

    // impl on duration
    fn get_appropriate_time_unit(&self, duration: Duration) -> String {
        if duration.as_nanos() < 1000 {
            format!("{} ns", duration.as_nanos())
        } else if duration.as_nanos() > 1000 && duration.as_millis() < 5 {
            format!("{} μs", duration.as_micros())
        } else if duration.as_micros() > 1000 && duration.as_millis() < 3000 {
            format!("{} ms", duration.as_millis())
        } else if duration.as_secs() <= 120 {
            format!("{:.2} s", duration.as_millis() as f64 / 1000.0)
        } else if duration.as_secs() <= 3600 {
            format!("{:.2} min", (duration.as_secs() as f64) / 60.0)
        } else {
            format!("{:.2} hours", (duration.as_secs() as f64) / 3600.0)
        }
    }
}

fn benchmark<T, F: Fn() -> T, V: FnMut(&mut T) -> bool>(
    setup: F,
    mut f: V,
) -> Result<BenchmarkResult> {
    // warmup run
    let mut val = setup();
    let intended_duration = Duration::from_millis(100);
    let mut iterations_per_check = 1;
    let mut iterations: usize = 0;
    let instant = Instant::now();

    // The reason for the "done" and boolean return type here is that some benchmarks may want to
    // finish earlier, e.g. random disk reads want to finish as soon as it's read every page since
    // otherwise we're just benchmarking memory. If this is the only use-case, maybe we should just
    // make sure this never happens.
    let mut done = false;
    while instant.elapsed() < intended_duration {
        for i in 1..(iterations_per_check + 1) {
            if !f(&mut val) {
                done = true;
                iterations_per_check = i;
                break;
            }
        }
        iterations += iterations_per_check;
        if done {
            break;
        }
    }

    let duration = Duration::from_secs(1);
    thread::sleep(duration);
    // real run
    let mut val = setup();
    let intended_duration = Duration::from_millis(5000);
    let mut iterations_per_check = iterations;
    let mut iterations: usize = 0;
    let instant = Instant::now();
    // let mut cycles = 0;

    // if cfg!(target_arch = "x86_64") {
    //     let rdtsc_before: u64;
    //     unsafe {
    //         rdtsc_before = core::arch::x86_64::_rdtsc();
    //     }
    // }

    let mut done = false;
    while instant.elapsed() < intended_duration {
        for i in 1..(iterations_per_check + 1) {
            // unlikely branch
            if !f(&mut val) {
                done = true;
                iterations_per_check = i;
                break;
            }
        }
        iterations += iterations_per_check;
        if done {
            break;
        }
    }
    let actual_duration = instant.elapsed();

    // if cfg!(target_arch = "x86_64") {
    //     let rdtsc_after: u64;
    //     unsafe {
    //         rdtsc_after = core::arch::x86_64::_rdtsc();
    //     }

    //     cycles = rdtsc_after - rdtsc_before
    // }

    Ok(BenchmarkResult {
        iterations,
        duration: actual_duration,
        // duration_ratio: intended_duration.as_nanos() as f64 / actual_duration.as_nanos() as f64,
        // intended_duration: intended_duration,
        cycles: 0,
    })
}

// TODO: take args for how long to perform tests
fn main() {
    let matches = App::new("Napkin Math")
        .version("0.1")
        .author("Simon Eskildsen <simon@sirupsen.com>")
        .about("Runs computing benchmarks to find numbers for napkin math.")
        .arg(
            Arg::new("number")
                .long("number")
                .short('n')
                .help("How many times to run each test")
                .value_name("N")
                .takes_value(true),
        )
        .arg(
            Arg::new("evaluate")
                .long("evaluate")
                .short('e')
                .help("Run tests that match a regex")
                .value_name("REGEX")
                .takes_value(true),
        )
        .get_matches();

    let methods: [(&'static str, fn()); 22] = [
        (
            "memory_read_sequential_threaded",
            memory_read_sequential_threaded,
        ),
        ("memory_read_sequential", memory_read_sequential),
        ("memory_write_sequential", memory_write_sequential),
        ("memory_read_random", memory_read_random),
        ("memory_write_random", memory_write_random),
        ("syscall_getpid", syscall_getpid),
        ("syscall_time", syscall_time),
        ("syscall_getrusage", syscall_getrusage),
        ("syscall_stat", syscall_stat),
        ("disk_read_sequential", disk_read_sequential),
        ("disk_read_random", disk_read_random),
        (
            "disk_write_sequential_no_fsync",
            disk_write_sequential_no_fsync,
        ),
        (
            "disk_read_sequential_io_uring",
            disk_read_sequential_io_uring,
        ),
        ("disk_write_sequential_fsync", disk_write_sequential_fsync),
        ("tcp_read_write", tcp_read_write),
        // ("simd", simd),
        ("redis_read_single_key", redis_read_single_key),
        ("mysql_write", mysql_write),
        ("sort", sort),
        ("mutex", mutex),
        ("hash_sha256", hash_sha256),
        ("hash_crc32", hash_crc32),
        ("hash_siphash", hash_siphash),
    ];

    if matches.occurrences_of("evaluate") > 0 {
        let regex_argument = matches.value_of("evaluate").unwrap_or(".*");
        println!("Matching tests with regex: {}", regex_argument);
        let regex = Regex::new(regex_argument).unwrap();
        let n = matches.value_of("number").unwrap_or("1");

        for (name, func) in &methods {
            if regex.is_match(name) {
                for _ in 0..(n.parse().unwrap_or(1)) {
                    println!("\nExecuting {}..", name);
                    func();
                }
            }
        }
    }
}

fn memory_write_sequential() {
    struct Test {
        i: usize,
        vec: Vec<[u64; 8]>,
    }
    let bytes_per_iteration = 64;
    let size_in_elements = (n_gb_bytes!(1) as u64 / bytes_per_iteration) as usize;

    let result = benchmark(
        || {
            let mut vec = Vec::new();
            vec.resize(size_in_elements, [1, 2, 3, 4, 5, 6, 7, 8]);
            Test { i: 0, vec }
        },
        |test| {
            test.vec[test.i] = [8, 7, 110_694, 5, 4, 3, 2, 1];
            black_box(test.vec[test.i]);
            test.i += 1;
            if test.i == test.vec.len() {
                return false;
            }
            true
        },
    )
    .unwrap();

    result.print_results("Write Seq Vec", 64);
}

fn memory_read_sequential() {
    struct Test {
        i: usize,
        vec: Vec<[u64; 8]>,
        total: u64,
    }

    let bytes_per_iteration = 64;
    let size_in_elements = (n_gb_bytes!(4) as u64 / bytes_per_iteration) as u64;

    // put these in separate functions so they can be disassembled.
    // #[inline] is going to be important here.
    let result = benchmark(
        || {
            let mut vec: Vec<[u64; 8]> = Vec::new();
            for i in 0..size_in_elements {
                vec.push([i, i, i, i, i, i, i, i]);
            }

            // println!("Length: {}", vec.len());
            Test {
                i: 0,
                vec,
                total: 0,
            }
        },
        |test| {
            if test.i == test.vec.len() {
                // println!("Result: {}", test.total);
                false
            } else {
                black_box(test.vec[test.i]);
                test.total += test.vec[test.i][0];
                test.i += 1;
                true
            }
        },
    )
    .unwrap();

    result.print_results("Read Seq Vec", bytes_per_iteration as usize);
}

// USE https://github.com/bheisler/criterion.rs instead... Just too much of a hassle to setup
// threads etc. and we should've done this from the get-go anyway.
fn memory_read_sequential_threaded() {
    let bytes_per_iteration = 64;
    let size_in_elements = (n_gb_bytes!(4) as u64 / bytes_per_iteration) as u64;

    let result = benchmark(
        || {
            let mut vec: Vec<[u64; 8]> = Vec::new();
            for i in 0..size_in_elements {
                vec.push([i, i, i, i, i, i, i, i]);
            }

            Arc::new(vec)
        },
        |vec| {
            let mut threads: Vec<thread::JoinHandle<_>> = vec![];
            let n_threads = 4;
            for k in 0..(n_threads) {
                let my_vec = vec.clone();
                let mut start = (my_vec.len() / n_threads) * k;
                let mut end = start + (my_vec.len() / n_threads);
                if k != 0 {
                    start += 1;
                    end += 1;
                }

                if k == n_threads - 1 {
                    end -= 1;
                }

                // println!("{}..{}", start, end);

                threads.push(thread::spawn(move || {
                    let mut i: u64 = 0;

                    for j in start..end {
                        i += my_vec[j][0];
                        black_box(my_vec[j]);
                    }

                    i
                }));
            }

            // let mut result = 0;
            for thread in threads {
                // result += thread.join().unwrap();
                thread.join().unwrap();
            }
            // print!("Result {}\n", result);

            false
        },
    )
    .unwrap();

    result.print_results("Read Seq Vec Threaded", n_gb_bytes!(4) as usize);
}

fn memory_write_random() {
    struct Test {
        vec: Vec<[u64; 8]>,
        order: Vec<usize>,
        i: usize,
    }

    let bytes_per_iteration = 64;
    let size_in_elements = (n_gb_bytes!(1) as u64 / bytes_per_iteration) as usize;

    let result = benchmark(
        || {
            let mut vec = Vec::new();
            vec.resize(size_in_elements, [1, 2, 3, 4, 5, 6, 7, 8]);

            let mut order: Vec<usize> = (0..size_in_elements).collect();
            order.shuffle(&mut thread_rng());
            Test { vec, order, i: 0 }
        },
        |test| {
            test.vec[test.order[test.i]] = [8, 7, 6, 5, 4, 3, 2, 1];
            black_box(test.vec[test.order[test.i]]);
            test.i += 1;
            if test.i == test.vec.len() {
                return false;
            }
            true
        },
    )
    .unwrap();

    result.print_results("Random Write Vec", bytes_per_iteration as usize);
}

struct MemoryReadTest {
    vec: Vec<[u64; 8]>,
    order: Vec<usize>,
    i: usize,
}

fn memory_read_random() {
    let result = benchmark(memory_read_random_setup, memory_read_random_iteration).unwrap();
    result.print_results("Random Read Vec", 64);
}

fn memory_read_random_setup() -> MemoryReadTest {
    let size_in_elements = (n_gb_bytes!(1) as u64 / 64) as usize;

    let mut vec = Vec::new();
    vec.resize(size_in_elements, [1, 2, 3, 4, 5, 6, 7, 8]);
    unsafe {
        let data = vec.as_mut_ptr() as *mut libc::c_void;
        libc::madvise(data, size_in_elements, libc::MADV_RANDOM);
    }

    let mut order: Vec<usize> = (0..size_in_elements).collect();
    unsafe {
        let data = order.as_mut_ptr() as *mut libc::c_void;
        libc::madvise(data, size_in_elements, libc::MADV_SEQUENTIAL);
    }
    order.shuffle(&mut thread_rng());
    MemoryReadTest { vec, order, i: 0 }
}

#[inline(always)]
fn memory_read_random_iteration(test: &mut MemoryReadTest) -> bool {
    black_box(test.vec[test.order[test.i]]);
    test.i += 1;
    if test.i == test.vec.len() {
        return false;
    }
    true
}

fn disk_write_sequential_fsync() {
    struct Test {
        bytes: Vec<u8>,
        file: std::fs::File,
    }

    let size_of_writes = n_kib_bytes!(8) as usize;

    let result = benchmark(
        || {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(FILE_NAME)
                .unwrap();

            let bytes: Vec<u8> = (0..size_of_writes).map(|_| rand::random::<u8>()).collect();

            Test { bytes, file }
        },
        |test| {
            test.file.write_all(&test.bytes).unwrap();
            test.file.sync_data().unwrap();
            true
        },
    )
    .unwrap();
    fs::remove_file(FILE_NAME).unwrap();

    result.print_results("Sequential Disk Write, Fsync", size_of_writes);
}

fn disk_write_sequential_no_fsync() {
    struct Test {
        bytes: Vec<u8>,
        file: std::fs::File,
    }

    let size_of_writes = n_kib_bytes!(8) as usize;

    let result = benchmark(
        || {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(FILE_NAME)
                .unwrap();

            let bytes: Vec<u8> = (0..size_of_writes).map(|_| rand::random::<u8>()).collect();

            Test { bytes, file }
        },
        |test| {
            test.file.write_all(&test.bytes).unwrap();
            true
        },
    )
    .unwrap();
    fs::remove_file(FILE_NAME).unwrap();

    result.print_results("Sequential Disk Write, No Fsync", size_of_writes);
}

fn disk_read_sequential() {
    const BUF_SIZE: usize = n_kib_bytes!(64) as usize;

    struct Test {
        buffer: [u8; BUF_SIZE],
        file: fs::File,
    }

    let result = benchmark(
        || {
            std::mem::drop(fs::remove_file(FILE_NAME));
            let buffer = vec![0; n_gib_bytes!(1) as usize];
            {
                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .read(true)
                    .open(FILE_NAME)
                    .unwrap();
                file.write_all(&buffer).unwrap();
                file.sync_data().unwrap();
            }

            let buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];
            let mut file = OpenOptions::new().read(true).open(FILE_NAME).unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();

            #[cfg(target_os = "linux")]
            unsafe {
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
            }

            // TODO: for linux
            if cfg!(target_os = "macos") {
                Command::new("sudo")
                    .arg("purge")
                    .output()
                    .expect("failed to flush page cache");
            }

            Test { buffer, file }
        },
        |test| {
            let n = test.file.read(&mut test.buffer).unwrap();
            // TODO: this is cheating...
            if n < BUF_SIZE {
                test.file.seek(SeekFrom::Start(0)).unwrap();
            };
            true
        },
    )
    .unwrap();
    fs::remove_file(FILE_NAME).unwrap();

    result.print_results("Sequential Disk Read", BUF_SIZE);
}

#[cfg(target_os = "macos")]
fn disk_read_sequential_io_uring() {
    println!("only supported on linux");
}

#[cfg(target_os = "linux")]
fn disk_read_sequential_io_uring() {
    // https://github.com/axboe/liburing/blob/master/examples/io_uring-cp.c
    const BUF_SIZE: usize = n_kib_bytes!(32) as usize;
    let reads_per_iteration: isize = 64;

    struct Test {
        buffers: Vec<Vec<u8>>,
        file: fs::File,
        ring: rio::Rio,
        size: usize,
        offset: usize,
    }
    use std::slice;

    // TODO: checksum somehow

    let result = benchmark(
        || {
            // flush page cache? prob not necessary since we re-create the file.
            let _ = fs::remove_file(FILE_NAME);
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .read(true)
                .open(FILE_NAME)
                .unwrap();
            let buffer = vec![0; n_gib_bytes!(1) as usize];
            file.write_all(&buffer).unwrap();
            file.sync_data().unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();

            // flush page cache after this

            unsafe {
                #[cfg(target_os = "linux")]
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
            }

            let ring = rio::new().expect("create uring");
            let buffers = vec![vec![0; BUF_SIZE]; reads_per_iteration as usize];
            Test {
                buffers,
                file,
                ring,
                size: n_gib_bytes!(1) as usize,
                offset: 0,
            }
        },
        |test| {
            let ptr = test.buffers.as_mut_ptr();
            let mut completions = vec![];

            for i in 0..reads_per_iteration {
                if test.size == 0 {
                    println!("Stopping early");
                    break;
                }

                unsafe {
                    let buf = &slice::from_raw_parts_mut(ptr.offset(i), 1)[0];
                    completions.push(test.ring.read_at(&test.file, buf, test.offset as u64));
                }

                test.offset += BUF_SIZE;
                test.size -= BUF_SIZE;
            }

            for completion in completions.into_iter() {
                let read = completion.wait().unwrap();
                if read < BUF_SIZE {
                    println!("at end?");
                }
            }

            if test.size == 0 {
                test.offset = 0;
                test.size = n_gib_bytes!(1) as usize;
            }

            true
        },
    )
    .unwrap();
    let _ = fs::remove_file(FILE_NAME);

    result.print_results(
        "Io-uring Sequential Disk Read",
        BUF_SIZE * (reads_per_iteration as usize),
    );
}

fn disk_read_random() {
    const BUF_SIZE: usize = n_kib_bytes!(8) as usize;

    struct Test {
        buffer: [u8; BUF_SIZE],
        pages: Vec<u64>,
        i: usize,
        file: std::fs::File,
    }
    let page_size = page_size::get();

    let result = benchmark(
        || {
            std::mem::drop(fs::remove_file(FILE_NAME));
            let buffer = vec![0; n_gib_bytes!(8) as usize];
            {
                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .read(true)
                    .open(FILE_NAME)
                    .unwrap();
                file.write_all(&buffer).unwrap();
                file.sync_data().unwrap();
            }

            let file = OpenOptions::new().read(true).open(FILE_NAME).unwrap();

            // This is to ensure we only visit each page once. Otherwise this is essentially just
            // benchmarking syscall + page cache, which is going to be awfully close to random
            // memory read.
            let mut pages: Vec<u64> = Vec::new();
            for i in 0..(buffer.len() / page_size) {
                pages.push((i * page_size + 1) as u64);
            }
            pages.shuffle(&mut thread_rng());

            #[cfg(target_os = "linux")]
            unsafe {
                libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_RANDOM);
            }

            // TODO: for linux
            if cfg!(target_os = "macos") {
                Command::new("sudo")
                    .arg("purge")
                    .output()
                    .expect("failed to flush page cache");
            }

            let buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];

            Test {
                file,
                pages,
                buffer,
                i: 0,
            }
        },
        |test| {
            test.file.seek(SeekFrom::Start(test.pages[test.i])).unwrap();
            test.file.read_exact(&mut test.buffer).unwrap();
            black_box(test.buffer);
            test.i += 1;

            if test.i == test.pages.len() {
                return false;
            };

            true
        },
    )
    .unwrap();
    fs::remove_file(FILE_NAME).unwrap();

    result.print_results("Random Disk Seek, No Page Cache", BUF_SIZE);
}

// this comes from the auxilirary vector on some OSes, making this not do a syscall.
// on the linux kernel I've been testing on, it does do a syscall. on darwin, it doesn't.
fn syscall_getpid() {
    use std::process;

    let result = benchmark(
        || {},
        |_test| {
            black_box(process::id());
            true
        },
    )
    .unwrap();
    result.print_results("Sycall getpid(2)", 0);
}

// this is available in user-space memory (depending on libc) and often doesn't result in a sycall.
fn syscall_time() {
    let result = benchmark(
        || {},
        |_test| {
            black_box(SystemTime::now());
            true
        },
    )
    .unwrap();
    result.print_results("Sycall gettimeofday(2)", 0);
}

// syscall, can't be optimized out
fn syscall_getrusage() {
    let time = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let rusage = Box::new(libc::rusage {
        ru_utime: time,
        ru_stime: time,
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    });
    let ptr = Box::into_raw(rusage);

    let result = benchmark(
        || {},
        |_test| {
            unsafe {
                libc::getrusage(0, ptr);
            }
            true
        },
    )
    .unwrap();
    result.print_results("Sycall getrusage(2)", 0);
}

// syscall, can't be optimized out
fn syscall_stat() {
    let f = fs::File::open("/tmp").unwrap();

    let result = benchmark(
        || {},
        |_test| {
            let metadata = f.metadata().unwrap();
            black_box(metadata);
            true
        },
    )
    .unwrap();
    result.print_results("Sycall stat(2)", 0);
}

fn tcp_read_write() {
    const BUF_SIZE: usize = n_kib_bytes!(64) as usize;

    // This server doesn't support multiple clients.
    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:8877").unwrap();
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();

            stream.set_nodelay(true).unwrap();
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(1000)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_millis(1000)))
                .unwrap();

            let mut buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];

            loop {
                match stream.read(&mut buffer) {
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // println!("s{}: failed to read, err: {:?}..", i, e);
                        continue;
                    }
                    Ok(n) => {
                        // println!("s{}: read: {}", i, n);

                        match stream.write(&buffer[..n]) {
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                // println!("s{}: failed to write", i);
                                continue;
                            }
                            Ok(_n) => {
                                // println!("s{}: write: {}", i, n);
                            }
                            Err(e) => panic!("{}", e),
                        };
                    }
                    Err(e) => panic!("{}", e),
                };

                // i += 1;
            }
        }
    });

    let bytes: Vec<u8> = (0..BUF_SIZE).map(|_| rand::random::<u8>()).collect();
    let mut buffer: [u8; BUF_SIZE] = [0; BUF_SIZE];

    // This is done outside the setup block to avoid having to deal with a shutdown signal..
    loop {
        match TcpStream::connect("127.0.0.1:8877") {
            Err(err) => {
                match err.kind() {
                    ErrorKind::ConnectionRefused => {
                        continue;
                    }
                    kind => panic!("Error occurred: {:?}", kind),
                };
            }
            Ok(mut stream) => {
                stream.set_nodelay(true).unwrap();
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_millis(1000)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_millis(1000)))
                    .unwrap();

                let result = benchmark(
                    || {},
                    |_| {
                        match stream.write(&bytes) {
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                // println!("c: failed to write");
                                return true;
                            }
                            Ok(n) => {
                                // println!("c: write: {}", n);

                                match stream.read(&mut buffer[0..n]) {
                                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                        // println!("c: failed to read, err: {:?}..", e);
                                        return true;
                                    }
                                    Ok(_n) => {
                                        // println!("c: read: {}\n", n);
                                    }
                                    Err(e) => {
                                        // println!("omgs read! {:?}", e.raw_os_error());
                                        panic!("{}", e);
                                    }
                                };
                            }
                            Err(e) => {
                                // println!("omgs write! {:?}", e.raw_os_error());
                                panic!("{}", e);
                            }
                        };

                        true
                    },
                )
                .unwrap();

                result.print_results("Tcp Echo", BUF_SIZE);
                break;
            }
        }
    }
}

// #[derive(Clone, Copy)]
// #[allow(non_camel_case_types)]
// pub union i32simd {
//     vector: __m256i,
//     numbers: [u32; 8],
// }

// fn simd() {
//     unsafe {
//         let a = i32simd {
//             vector: _mm256_set_epi32(1, 2, 3, 4, 5, 6, 7, 8),
//         };
//         let b = i32simd {
//             vector: _mm256_set_epi32(1, 2, 3, 4, 5, 6, 7, 8),
//         };
//         let result = i32simd {
//             vector: _mm256_mul_epi32(a.vector, b.vector),
//         };
//         let result2 = i32simd {
//             vector: _mm256_mullo_epi32(a.vector, b.vector),
//         };
//         println!("{:?}", result.numbers);
//         println!("{:?}", result2.numbers);
//     }
// }

fn redis_read_single_key() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();

    let result = benchmark(
        || {
            let mut con = client.get_connection().unwrap();
            let bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
            con.set::<&str, Vec<u8>, ()>("1", bytes).unwrap();
            con
        },
        |con| {
            std::mem::drop::<Vec<u8>>(con.get("1").unwrap());
            true
        },
    )
    .unwrap();

    result.print_results("Redis Read", 64);
}

fn sort() {
    const TOTAL_SIZE: usize = n_mib_bytes!(1) as usize;

    let result = benchmark(
        || {
            let elements = TOTAL_SIZE / 8;
            let bytes: Vec<u64> = (0..elements).map(|_| rand::random::<u64>()).collect();
            bytes
        },
        |bytes| {
            bytes.sort_unstable();
            // TODO: enum to re-start with setup or stop entirely
            false
        },
    )
    .unwrap();

    result.print_results("Sort", TOTAL_SIZE);
}

fn mutex() {
    let mutex = Arc::new(Mutex::new(0));

    let result = benchmark(
        || {
            let t_mutex = mutex.clone();
            thread::spawn(move || {
                loop {
                    let mut data = t_mutex.lock().unwrap();
                    // let duration = time::Duration::from_micros(10);
                    // thread::sleep(duration);
                    *data += 10;
                }
            });

            mutex.clone()
        },
        |mutex| {
            let mut data = mutex.lock().unwrap();
            *data += 10;
            true
        },
    )
    .unwrap();

    println!("{}", mutex.lock().unwrap());
    result.print_results("Mutex", 1);
}

fn hash_sha256() {
    let size_of_writes = 64;

    let result = benchmark(
        || {
            let bytes: Vec<u8> = (0..size_of_writes).map(|_| rand::random::<u8>()).collect();
            bytes
        },
        |bytes| {
            black_box(Sha256::digest(bytes));
            true
        },
    )
    .unwrap();

    result.print_results("Sha256", size_of_writes);
}

fn hash_crc32() {
    use crc32fast::Hasher;
    let size_of_writes = 64;

    let result = benchmark(
        || {
            let bytes: Vec<u8> = (0..size_of_writes).map(|_| rand::random::<u8>()).collect();
            bytes
        },
        |bytes| {
            let mut hasher = Hasher::new();
            hasher.update(bytes);
            black_box(hasher.finalize());
            true
        },
    )
    .unwrap();

    result.print_results("CRC32", size_of_writes);
}

fn hash_siphash() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let size_of_writes = 64;

    let result = benchmark(
        || {
            let bytes: Vec<u8> = (0..size_of_writes).map(|_| rand::random::<u8>()).collect();
            bytes
        },
        |bytes| {
            let mut hasher = DefaultHasher::new();
            hasher.write(bytes);
            black_box(hasher.finish());
            true
        },
    )
    .unwrap();

    result.print_results("SIPHash", size_of_writes);
}

fn mysql_write() {
    let url = "mysql://root:@localhost:3306/napkin";

    // struct Product {
    //     id: i64,
    //     shop_id: i64,
    //     title: Option<String>,
    //     body_html: Option<String>,
    //     vendor: Option<String>,
    //     created_at: SystemTime,
    //     updated_at: SystemTime,
    // }

    // https://mydbops.wordpress.com/2018/07/27/innodb-physical-files-on-mysql-8-0/
    // 0x5 => 5 => ib_logfile0: REDO LOG
    // 0x1C => 28 => binlog: BIN LOG
    // 0x9 => 9 => ibdata: SHARED TABLESPACE
    // 0x20 => 32 => products.ibd: TABLE ITSELF
    // 0xD => 13 => undo
    // 0x19 => temp_5.ibt
    //
    // I think some are slow, some fast, due to filesystem batching?
    // https://www.kernel.org/doc/Documentation/filesystems/ext4.txt
    //
    // This is an `strace` for a MYSQL insert. Notice the huge variability in elapsed time (in
    // microseconds). You can see the 2PC, then the later stages (after the transaction returns) to
    // update InnoDB (table, double-write buffer, etc.)
    //
    // Is the variability due to EXT4 batching? max_batch_time / min_batch_time mounting options
    // (defaults are 0..15ms)
    // https://www.kernel.org/doc/Documentation/filesystems/ext4.txt
    // If we trace the kernel here for a backtrace in the fsync path in ext4
    // (http://www.brendangregg.com/blog/2016-01-18/ebpf-stack-trace-hack.html) we would likely get
    // this.
    //
    // PID/THRD        RELATIVE  ELAPSD    CPU SYSCALL(args)
    // 4020/0x15e84b:  43494372    5536     99 fsync(0x5, 0x0, 0x0)   // PREPARE, SLOW
    // 4020/0xa145e6:    535406     267     93 fsync(0x1C, 0x0, 0x0)  // PREPARE, FAST
    // 4020/0x15e84b:  43494492     238     87 fsync(0x5, 0x0, 0x0)   // ?, FAST
    // 4020/0x15e84b:  43495108    2796    129 fsync(0x5, 0x0, 0x0)   // COMMIT, SLOW
    //
    // I think all this happens after client has returned..?
    //
    // 4020/0x15e7a7:  14185246    5577    147 fsync(0x9, 0x0, 0x0)  // ?
    // 4020/0x15e7a3:     81974     277     92 fsync(0x9, 0x0, 0x0)  // ?
    // 4020/0x15e7a3:     82035     216     52 fsync(0x20, 0x0, 0x0) // FLUSH
    // 4020/0x15e7a3:     82116     231     61 fsync(0xD, 0x0, 0x0)  // ?
    // 4020/0x15e84b:  43495999     249     86 fsync(0x5, 0x0, 0x0)  // ?
    // 4020/0x15e848:  130442427     195     37 fsync(0x5, 0x0, 0x0) // ?
    // 4020/0x15e7a7:  14185565    5626     89 fsync(0x9, 0x0, 0x0)  // ?
    // 4020/0x15e7a3:     82274     359     90 fsync(0x19, 0x0, 0x0) //
    // 4020/0x15e848:  130444415     277    110 fsync(0x5, 0x0, 0x0)
    let result = benchmark(
        || {
            let opts = Opts::from_url(url).unwrap();
            let pool = Pool::new(opts).unwrap();
            let mut conn = pool.get_conn().unwrap();
            conn.query_drop(
                r"
                DROP TABLE IF EXISTS products;
            ",
            )
            .unwrap();

            conn.query_drop(
                r"
                CREATE TABLE IF NOT EXISTS `products` (
                  `id` bigint(20) NOT NULL AUTO_INCREMENT,
                  `shop_id` bigint(20) DEFAULT NULL,
                  `title` varchar(255) DEFAULT NULL,
                  `body_html` mediumtext,
                  `vendor` varchar(255) DEFAULT NULL,
                  `created_at` datetime DEFAULT NULL,
                  `updated_at` datetime DEFAULT NULL,
                  PRIMARY KEY (`id`)
                ) ENGINE=InnoDB AUTO_INCREMENT=0 DEFAULT CHARSET=utf8mb4 ROW_FORMAT=DYNAMIC
            ",
            )
            .unwrap();
            pool
        },
        |pool| {
            let mut handles = vec![];

            // Why is this faster than fsync(2)?
            //
            // (1) Concurrent fsyncs to multiple disks...?
            // (2) Group Commit?
            //
            // For some reason, some of these fsyncs are taking < 1ms, wheras in my benchmarks they
            // typically take 5ms (which also does happen). Extremely variable.
            for i in 0..16 {
                println!("thread: {}", i);
                handles.push(thread::spawn({
                    let pool = pool.clone();
                    move || {
                        let mut conn = pool.get_conn().unwrap();
                        for _ in 0..1000 {
                            conn.exec_drop(
                                r"INSERT INTO products (shop_id, title) VALUES (:shop_id, :title)",
                                params! { "shop_id" => 123, "title" => "aerodynamic chair" },
                            )
                            .unwrap();
                        }
                    }
                }));
            }
            // Expected 'naive' fsyncs to the binlog: 16 * 1,000 => 16,000
            //
            // Actual as per `sudo dtruss -e -n mysql -t fsync 2>&1 | grep "fsync(0x1C"`:
            //
            // 71 entries!

            for handle in handles {
                handle.join().unwrap();
            }
            false
        },
    )
    .unwrap();

    result.print_results("MySQL Write", 8 + 17);
}*/