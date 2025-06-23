# Memory errors discovered (double checked with Valgrind)

## 1st repository
### name: noGenerator
### link: https://github.com/pamquale/noGenerator
### type: 1 memory leak
### notes: none, 2021
### valgrind output:
```bash
==51070== Memcheck, a memory error detector
==51070== Copyright (C) 2002-2017, and GNU GPL'd, by Julian Seward et al.
==51070== Using Valgrind-3.18.1 and LibVEX; rerun with -h for copyright info
==51070== Command: ./target/debug/noGenerator
==51070== 
41
==51070== 
==51070== HEAP SUMMARY:
==51070==     in use at exit: 28 bytes in 2 blocks
==51070==   total heap usage: 11 allocs, 9 frees, 3,132 bytes allocated
==51070== 
==51070== 4 bytes in 1 blocks are indirectly lost in loss record 1 of 2
==51070==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51070==    by 0x11EE7C: alloc::alloc::alloc (alloc.rs:99)
==51070==    by 0x11EFBC: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51070==    by 0x11EDB0: alloc::alloc::exchange_malloc (alloc.rs:257)
==51070==    by 0x11E923: noGenerator::main (main.rs:1)
==51070==    by 0x11E2FA: core::ops::function::FnOnce::call_once (function.rs:250)
==51070==    by 0x1203FD: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51070==    by 0x11FB40: std::rt::lang_start::{{closure}} (rt.rs:195)
==51070==    by 0x13C2F6: std::rt::lang_start_internal (function.rs:284)
==51070==    by 0x11FB19: std::rt::lang_start (rt.rs:194)
==51070==    by 0x11EC4D: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/noGenerator/target/debug/noGenerator)
==51070== 
==51070== 28 (24 direct, 4 indirect) bytes in 1 blocks are definitely lost in loss record 2 of 2
==51070==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51070==    by 0x11EE7C: alloc::alloc::alloc (alloc.rs:99)
==51070==    by 0x11EFBC: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51070==    by 0x11EDB0: alloc::alloc::exchange_malloc (alloc.rs:257)
==51070==    by 0x11E962: noGenerator::main (boxed.rs:254)
==51070==    by 0x11E2FA: core::ops::function::FnOnce::call_once (function.rs:250)
==51070==    by 0x1203FD: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51070==    by 0x11FB40: std::rt::lang_start::{{closure}} (rt.rs:195)
==51070==    by 0x13C2F6: std::rt::lang_start_internal (function.rs:284)
==51070==    by 0x11FB19: std::rt::lang_start (rt.rs:194)
==51070==    by 0x11EC4D: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/noGenerator/target/debug/noGenerator)
==51070== 
==51070== LEAK SUMMARY:
==51070==    definitely lost: 24 bytes in 1 blocks
==51070==    indirectly lost: 4 bytes in 1 blocks
==51070==      possibly lost: 0 bytes in 0 blocks
==51070==    still reachable: 0 bytes in 0 blocks
==51070==         suppressed: 0 bytes in 0 blocks
==51070== 
==51070== ERROR SUMMARY: 1 errors from 1 contexts (suppressed: 0 from 0)
```

## 2nd repository
### name: wasm-demo
### link: https://github.com/danpaz/wasm-demo
### type: 1 memory leak
### note: no invocation of functions defined in the main, so intra-procedural analysis.
### how to replicate: 
```bash
cargo run ../cargo_project_test/found\ vulns/wasm-demo -f "rust::demo_new_state::bb0"
```
### valgrind output: **N.B.:** to replicate uncomment the target function call in the main in the main.rs 
```bash
==49113== 
==49113== HEAP SUMMARY:
==49113==     in use at exit: 804 bytes in 201 blocks
==49113==   total heap usage: 209 allocs, 8 frees, 3,900 bytes allocated
==49113== 
==49113== 804 bytes in 201 blocks are definitely lost in loss record 1 of 1
==49113==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==49113==    by 0x11D3EC: alloc::alloc::alloc (alloc.rs:99)
==49113==    by 0x11D52C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==49113==    by 0x11D320: alloc::alloc::exchange_malloc (alloc.rs:257)
==49113==    by 0x11D798: demo_new_state (boxed.rs:254)
==49113==    by 0x11D8A3: wasm_demo::main (main.rs:38)
==49113==    by 0x11D73A: core::ops::function::FnOnce::call_once (function.rs:250)
==49113==    by 0x11D99D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==49113==    by 0x11D970: std::rt::lang_start::{{closure}} (rt.rs:195)
==49113==    by 0x138EE6: std::rt::lang_start_internal (function.rs:284)
==49113==    by 0x11D949: std::rt::lang_start (rt.rs:194)
==49113==    by 0x11D8CD: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/wasm-demo/target/debug/wasm-demo)
==49113== 
==49113== LEAK SUMMARY:
==49113==    definitely lost: 804 bytes in 201 blocks
==49113==    indirectly lost: 0 bytes in 0 blocks
==49113==      possibly lost: 0 bytes in 0 blocks
==49113==    still reachable: 0 bytes in 0 blocks
==49113==         suppressed: 0 bytes in 0 blocks
==49113== 
==49113== ERROR SUMMARY: 1 errors from 1 contexts (suppressed: 0 from 0)
```
 
## 3rd repository 
### name: skip-list-test
### link: https://github.com/abhijeetbhagat/skip-list-test
### type: 10 memory leak
### note: no invocation of functions defined in the main, so intra-procedural analysis, 2021
### valgrind output:
```bash
==51185== Memcheck, a memory error detector
==51185== Copyright (C) 2002-2017, and GNU GPL'd, by Julian Seward et al.
==51185== Using Valgrind-3.18.1 and LibVEX; rerun with -h for copyright info
==51185== Command: ./target/debug/skip-list-test
==51185== 
thread 'main' panicked at src/main.rs:63:17:
index out of bounds: the len is 0 but the index is 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
==51185== 
==51185== HEAP SUMMARY:
==51185==     in use at exit: 360 bytes in 12 blocks
==51185==   total heap usage: 23 allocs, 11 frees, 2,608 bytes allocated
==51185== 
==51185== 8 bytes in 1 blocks are indirectly lost in loss record 1 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11F018: skip_list_test::main (main.rs:59)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are indirectly lost in loss record 2 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11E8A1: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 3 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11E963: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 4 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EA25: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 5 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EAE7: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 6 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EBA9: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 7 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EC6B: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 8 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11ED2D: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 9 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EDEF: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 10 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EEB1: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 32 bytes in 1 blocks are definitely lost in loss record 11 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11EF70: skip_list_test::main (boxed.rs:254)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== 72 (32 direct, 40 indirect) bytes in 1 blocks are definitely lost in loss record 12 of 12
==51185==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51185==    by 0x11DE1C: alloc::alloc::alloc (alloc.rs:99)
==51185==    by 0x11DF5C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51185==    by 0x11DD50: alloc::alloc::exchange_malloc (alloc.rs:257)
==51185==    by 0x11E65C: new<skip_list_test::Node<u32>> (boxed.rs:254)
==51185==    by 0x11E65C: skip_list_test::List<T>::new (main.rs:22)
==51185==    by 0x11F003: skip_list_test::main (main.rs:58)
==51185==    by 0x11D6EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51185==    by 0x11D80D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51185==    by 0x11E110: std::rt::lang_start::{{closure}} (rt.rs:195)
==51185==    by 0x13AD96: std::rt::lang_start_internal (function.rs:284)
==51185==    by 0x11E0E9: std::rt::lang_start (rt.rs:194)
==51185==    by 0x11F7ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/skip-list-test/target/debug/skip-list-test)
==51185== 
==51185== LEAK SUMMARY:
==51185==    definitely lost: 320 bytes in 10 blocks
==51185==    indirectly lost: 40 bytes in 2 blocks
==51185==      possibly lost: 0 bytes in 0 blocks
==51185==    still reachable: 0 bytes in 0 blocks
==51185==         suppressed: 0 bytes in 0 blocks
==51185== 
==51185== ERROR SUMMARY: 10 errors from 10 contexts (suppressed: 0 from 0)
```

## 4th repository
### name: rusant
### link: https://github.com/JakWai01/rusant
### type: 1 memory leak
### note: main() -> open url
- ```rust CString::new("").unwrap().into_raw()```
allocates a buffer (even if empty) and gives the pointer to the lib santapanelo/bridge:

- no ```c free()``` instructions on that pointer
- ```rust into_raw``` transfers the ownership: buffer never freed by rust Rust, the C/Go caller should invoke the ```c free()```
- In the bridge code there's no free a ```c free(ptr)``` after passed the value to Go, so the pointer is still alive
### valgrind output:
```bash
==51281== Memcheck, a memory error detector
==51281== Copyright (C) 2002-2017, and GNU GPL'd, by Julian Seward et al.
==51281== Using Valgrind-3.18.1 and LibVEX; rerun with -h for copyright info
==51281== Command: ./target/debug/rusant
==51281== 
open_url returned: ""
==51281== 
==51281== HEAP SUMMARY:
==51281==     in use at exit: 1 bytes in 1 blocks
==51281==   total heap usage: 19 allocs, 18 frees, 3,630 bytes allocated
==51281== 
==51281== 1 bytes in 1 blocks are definitely lost in loss record 1 of 1
==51281==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51281==    by 0x172620: <&str as alloc::ffi::c_str::CString::new::SpecNewImpl>::spec_new_impl (alloc.rs:99)
==51281==    by 0x12A52A: alloc::ffi::c_str::CString::new (c_str.rs:315)
==51281==    by 0x126767: rusant::open_url (main.rs:10)
==51281==    by 0x126957: rusant::main (main.rs:20)
==51281==    by 0x12564A: core::ops::function::FnOnce::call_once (function.rs:250)
==51281==    by 0x12C0ED: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51281==    by 0x12B7C0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51281==    by 0x14E756: std::rt::lang_start_internal (function.rs:284)
==51281==    by 0x12B799: std::rt::lang_start (rt.rs:194)
==51281==    by 0x126B4D: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/rusant/target/debug/rusant)
==51281== 
==51281== LEAK SUMMARY:
==51281==    definitely lost: 1 bytes in 1 blocks
==51281==    indirectly lost: 0 bytes in 0 blocks
==51281==      possibly lost: 0 bytes in 0 blocks
==51281==    still reachable: 0 bytes in 0 blocks
==51281==         suppressed: 0 bytes in 0 blocks
==51281== 
==51281== ERROR SUMMARY: 1 errors from 1 contexts (suppressed: 0 from 0)
```

## 5th repository
### name: napkin-math
### link: https://github.com/sirupsen/napkin-math/
### type: 1 memory leak
### note: syscall_getrusage never wraps back the raw pointer after Box::into_raw(). Since the repo uses jemalloc, modified with the global allocator to be analyzed with Valgrind
### valgrind output:
```bash
==51438== Memcheck, a memory error detector
==51438== Copyright (C) 2002-2017, and GNU GPL'd, by Julian Seward et al.
==51438== Using Valgrind-3.18.1 and LibVEX; rerun with -h for copyright info
==51438== Command: ./target/debug/napkin-math-test
==51438== 
[Sycall getrusage(2)] Iterations: 1
[Sycall getrusage(2)] Duration (ms): 0
Leaked rusage at: 0x4abda30
==51438== 
==51438== HEAP SUMMARY:
==51438==     in use at exit: 144 bytes in 1 blocks
==51438==   total heap usage: 9 allocs, 8 frees, 3,240 bytes allocated
==51438== 
==51438== 144 bytes in 1 blocks are definitely lost in loss record 1 of 1
==51438==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51438==    by 0x11EC0C: alloc::alloc::alloc (alloc.rs:99)
==51438==    by 0x11ED4C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51438==    by 0x11EB40: alloc::alloc::exchange_malloc (alloc.rs:257)
==51438==    by 0x11E49E: napkin_math_test::syscall_getrusage (boxed.rs:254)
==51438==    by 0x11E6B5: napkin_math_test::main (main.rs:79)
==51438==    by 0x11EA0A: core::ops::function::FnOnce::call_once (function.rs:250)
==51438==    by 0x11EA5D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51438==    by 0x11E950: std::rt::lang_start::{{closure}} (rt.rs:195)
==51438==    by 0x13A3B6: std::rt::lang_start_internal (function.rs:284)
==51438==    by 0x11E929: std::rt::lang_start (rt.rs:194)
==51438==    by 0x11E6ED: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/napkin-math-test/target/debug/napkin-math-test)
==51438== 
==51438== LEAK SUMMARY:
==51438==    definitely lost: 144 bytes in 1 blocks
==51438==    indirectly lost: 0 bytes in 0 blocks
==51438==      possibly lost: 0 bytes in 0 blocks
==51438==    still reachable: 0 bytes in 0 blocks
==51438==         suppressed: 0 bytes in 0 blocks
==51438== 
==51438== ERROR SUMMARY: 1 errors from 1 contexts (suppressed: 0 from 0)
```

## 6th repository
### name: shared-register
### link: https://github.com/namrapatel/shared-register
### type: 1 memory leak
### note: main Box::into_raw "rust::main::bb9"
### valgrind output:
```bash
==51605== Memcheck, a memory error detector
==51605== Copyright (C) 2002-2017, and GNU GPL'd, by Julian Seward et al.
==51605== Using Valgrind-3.18.1 and LibVEX; rerun with -h for copyright info
==51605== Command: ./target/debug/shared-register
==51605== 
Please provide a port number as an argument
==51605== 
==51605== HEAP SUMMARY:
==51605==     in use at exit: 24 bytes in 1 blocks
==51605==   total heap usage: 19 allocs, 18 frees, 3,510 bytes allocated
==51605== 
==51605== 24 bytes in 1 blocks are definitely lost in loss record 1 of 1
==51605==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51605==    by 0x31B38C: alloc::alloc::alloc (alloc.rs:99)
==51605==    by 0x31B4CC: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51605==    by 0x31B2C0: alloc::alloc::exchange_malloc (alloc.rs:257)
==51605==    by 0x326423: shared_register::main (boxed.rs:254)
==51605==    by 0x31D03A: core::ops::function::FnOnce::call_once (function.rs:250)
==51605==    by 0x31A29D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51605==    by 0x31DFC0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51605==    by 0x726716: std::rt::lang_start_internal (function.rs:284)
==51605==    by 0x31DF99: std::rt::lang_start (rt.rs:194)
==51605==    by 0x3269BD: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/shared-register/target/debug/shared-register)
==51605== 
==51605== LEAK SUMMARY:
==51605==    definitely lost: 24 bytes in 1 blocks
==51605==    indirectly lost: 0 bytes in 0 blocks
==51605==      possibly lost: 0 bytes in 0 blocks
==51605==    still reachable: 0 bytes in 0 blocks
==51605==         suppressed: 0 bytes in 0 blocks
==51605== 
==51605== ERROR SUMMARY: 1 errors from 1 contexts (suppressed: 0 from 0)
```
## 7th repository
### name: whisper-rs-example
### link: https://github.com/bruceunx/whisper-rs-example
### type: 1 memory leak
### note: into raw msg sender -  let sender_ptr = Box::into_raw(Box::new(tx)) as *mut std::ffi::c_void;
### valgrind output: **N.B.:**: the second error in "possibly lost" is related to chunks still reachable through thread internals that never got cleaned up (NOT ADDRESSED CREMA).
```bash
==51732== 
==51732== HEAP SUMMARY:
==51732==     in use at exit: 1,016 bytes in 7 blocks
==51732==   total heap usage: 129 allocs, 122 frees, 89,052 bytes allocated
==51732== 
==51732== 16 bytes in 1 blocks are still reachable in loss record 1 of 7
==51732==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x1E55B4: std::sys::pal::unix::thread::Thread::new (alloc.rs:99)
==51732==    by 0x146C67: std::thread::Builder::spawn_unchecked_ (mod.rs:600)
==51732==    by 0x1461AF: std::thread::Builder::spawn_unchecked (mod.rs:467)
==51732==    by 0x14614B: std::thread::spawn (mod.rs:400)
==51732==    by 0x153535: rust_iwhisper_example::main (main.rs:58)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732==    by 0x144EF0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51732==    by 0x1DC796: std::rt::lang_start_internal (function.rs:284)
==51732==    by 0x144EC9: std::rt::lang_start (rt.rs:194)
==51732==    by 0x153ADD: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/whisper-rs-example/target/debug/rust-iwhisper-example)
==51732== 
==51732== 16 bytes in 1 blocks are definitely lost in loss record 2 of 7
==51732==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x15C90C: alloc::alloc::alloc (alloc.rs:99)
==51732==    by 0x15CA4C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51732==    by 0x15C840: alloc::alloc::exchange_malloc (alloc.rs:257)
==51732==    by 0x1535B7: rust_iwhisper_example::main (boxed.rs:254)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732==    by 0x144EF0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51732==    by 0x1DC796: std::rt::lang_start_internal (function.rs:284)
==51732==    by 0x144EC9: std::rt::lang_start (rt.rs:194)
==51732==    by 0x153ADD: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/whisper-rs-example/target/debug/rust-iwhisper-example)
==51732== 
==51732== 48 bytes in 1 blocks are still reachable in loss record 3 of 7
==51732==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x1DDA8D: std::thread::Thread::new_unnamed (alloc.rs:99)
==51732==    by 0x146512: std::thread::Builder::spawn_unchecked_ (mod.rs:503)
==51732==    by 0x1461AF: std::thread::Builder::spawn_unchecked (mod.rs:467)
==51732==    by 0x14614B: std::thread::spawn (mod.rs:400)
==51732==    by 0x153535: rust_iwhisper_example::main (main.rs:58)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732==    by 0x144EF0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51732==    by 0x1DC796: std::rt::lang_start_internal (function.rs:284)
==51732==    by 0x144EC9: std::rt::lang_start (rt.rs:194)
==51732==    by 0x153ADD: main (in /home/af/Documenti/a-phd/cargo_project_test/found vulns/whisper-rs-example/target/debug/rust-iwhisper-example)
==51732== 
==51732== 48 bytes in 1 blocks are still reachable in loss record 4 of 7
==51732==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x15C90C: alloc::alloc::alloc (alloc.rs:99)
==51732==    by 0x15CA4C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51732==    by 0x15C840: alloc::alloc::exchange_malloc (alloc.rs:257)
==51732==    by 0x14678F: new<alloc::sync::ArcInner<std::thread::Packet<()>>> (boxed.rs:254)
==51732==    by 0x14678F: new<std::thread::Packet<()>> (sync.rs:391)
==51732==    by 0x14678F: std::thread::Builder::spawn_unchecked_ (mod.rs:514)
==51732==    by 0x1461AF: std::thread::Builder::spawn_unchecked (mod.rs:467)
==51732==    by 0x14614B: std::thread::spawn (mod.rs:400)
==51732==    by 0x153535: rust_iwhisper_example::main (main.rs:58)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732==    by 0x144EF0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51732==    by 0x1DC796: std::rt::lang_start_internal (function.rs:284)
==51732== 
==51732== 72 bytes in 1 blocks are still reachable in loss record 5 of 7
==51732==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x15C90C: alloc::alloc::alloc (alloc.rs:99)
==51732==    by 0x15CA4C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51732==    by 0x15C840: alloc::alloc::exchange_malloc (alloc.rs:257)
==51732==    by 0x146B13: new<std::thread::{impl#0}::spawn_unchecked_::{closure_env#1}<rust_iwhisper_example::main::{closure_env#0}, ()>> (boxed.rs:254)
==51732==    by 0x146B13: std::thread::Builder::spawn_unchecked_ (mod.rs:580)
==51732==    by 0x1461AF: std::thread::Builder::spawn_unchecked (mod.rs:467)
==51732==    by 0x14614B: std::thread::spawn (mod.rs:400)
==51732==    by 0x153535: rust_iwhisper_example::main (main.rs:58)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732==    by 0x144EF0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51732==    by 0x1DC796: std::rt::lang_start_internal (function.rs:284)
==51732== 
==51732== 304 bytes in 1 blocks are possibly lost in loss record 6 of 7
==51732==    at 0x484DA83: calloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x40147D9: calloc (rtld-malloc.h:44)
==51732==    by 0x40147D9: allocate_dtv (dl-tls.c:375)
==51732==    by 0x40147D9: _dl_allocate_tls (dl-tls.c:634)
==51732==    by 0x4C3A7B4: allocate_stack (allocatestack.c:430)
==51732==    by 0x4C3A7B4: pthread_create@@GLIBC_2.34 (pthread_create.c:647)
==51732==    by 0x1E56A1: std::sys::pal::unix::thread::Thread::new (thread.rs:84)
==51732==    by 0x146C67: std::thread::Builder::spawn_unchecked_ (mod.rs:600)
==51732==    by 0x1461AF: std::thread::Builder::spawn_unchecked (mod.rs:467)
==51732==    by 0x14614B: std::thread::spawn (mod.rs:400)
==51732==    by 0x153535: rust_iwhisper_example::main (main.rs:58)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732==    by 0x144EF0: std::rt::lang_start::{{closure}} (rt.rs:195)
==51732==    by 0x1DC796: std::rt::lang_start_internal (function.rs:284)
==51732== 
==51732== 512 bytes in 1 blocks are still reachable in loss record 7 of 7
==51732==    at 0x484DE30: memalign (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x484DF92: posix_memalign (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==51732==    by 0x1E2E0D: __rdl_alloc (unix.rs:85)
==51732==    by 0x15C90C: alloc::alloc::alloc (alloc.rs:99)
==51732==    by 0x15CA4C: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==51732==    by 0x15C840: alloc::alloc::exchange_malloc (alloc.rs:257)
==51732==    by 0x140DC8: std::sync::mpmc::counter::new (boxed.rs:254)
==51732==    by 0x145F3E: std::sync::mpmc::channel (mod.rs:198)
==51732==    by 0x15ACF7: std::sync::mpsc::channel (mod.rs:526)
==51732==    by 0x1534C3: rust_iwhisper_example::main (main.rs:56)
==51732==    by 0x1567EA: core::ops::function::FnOnce::call_once (function.rs:250)
==51732==    by 0x14856D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==51732== 
==51732== LEAK SUMMARY:
==51732==    definitely lost: 16 bytes in 1 blocks
==51732==    indirectly lost: 0 bytes in 0 blocks
==51732==      possibly lost: 304 bytes in 1 blocks
==51732==    still reachable: 696 bytes in 5 blocks
==51732==         suppressed: 0 bytes in 0 blocks
==51732== 
==51732== ERROR SUMMARY: 2 errors from 2 contexts (suppressed: 0 from 0)
```
## 8th repository
### name: lock-free
### link: https://github.com/DorukCem/lock-free
### type: 1 memory leak
### how to replicate: cargo run ../target_repos/lock-free -f "rust::LockFreeStack::<T>::push::bb0"
### valgrind output: 
```bash
==90776== 
==90776== HEAP SUMMARY:
==90776==     in use at exit: 159,992 bytes in 10,000 blocks
==90776==   total heap usage: 10,101 allocs, 101 frees, 170,656 bytes allocated
==90776== 
==90776== 159,984 bytes in 9,999 blocks are indirectly lost in loss record 1 of 2
==90776==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==90776==    by 0x12868C: alloc::alloc::alloc (alloc.rs:99)
==90776==    by 0x1287CC: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==90776==    by 0x1285C0: alloc::alloc::exchange_malloc (alloc.rs:257)
==90776==    by 0x129B73: new<lock_free::Node<i32>> (boxed.rs:254)
==90776==    by 0x129B73: lock_free::LockFreeStack<T>::push (main.rs:25)
==90776==    by 0x129E7F: lock_free::main::{{closure}}::{{closure}} (main.rs:89)
==90776==    by 0x126941: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==90776==    by 0x128269: std::thread::Builder::spawn_unchecked_::{{closure}}::{{closure}} (mod.rs:561)
==90776==    by 0x128A1F: <core::panic::unwind_safe::AssertUnwindSafe<F> as core::ops::function::FnOnce<()>>::call_once (unwind_safe.rs:272)
==90776==    by 0x12931F: std::panicking::try::do_call (panicking.rs:557)
==90776==    by 0x12835A: __rust_try (in /home/af/Documenti/a-phd/target_repos/lock-free/target/debug/lock-free)
==90776==    by 0x127E3E: std::thread::Builder::spawn_unchecked_::{{closure}} (panicking.rs:520)
==90776== 
==90776== 159,992 (8 direct, 159,984 indirect) bytes in 1 blocks are definitely lost in loss record 2 of 2
==90776==    at 0x4848899: malloc (in /usr/libexec/valgrind/vgpreload_memcheck-amd64-linux.so)
==90776==    by 0x12868C: alloc::alloc::alloc (alloc.rs:99)
==90776==    by 0x1287CC: alloc::alloc::Global::alloc_impl (alloc.rs:195)
==90776==    by 0x1285C0: alloc::alloc::exchange_malloc (alloc.rs:257)
==90776==    by 0x121F3A: lock_free::main (boxed.rs:254)
==90776==    by 0x122B5A: core::ops::function::FnOnce::call_once (function.rs:250)
==90776==    by 0x12691D: std::sys::backtrace::__rust_begin_short_backtrace (backtrace.rs:154)
==90776==    by 0x1289C0: std::rt::lang_start::{{closure}} (rt.rs:195)
==90776==    by 0x146D26: std::rt::lang_start_internal (function.rs:284)
==90776==    by 0x128999: std::rt::lang_start (rt.rs:194)
==90776==    by 0x12235D: main (in /home/af/Documenti/a-phd/target_repos/lock-free/target/debug/lock-free)
==90776== 
==90776== LEAK SUMMARY:
==90776==    definitely lost: 8 bytes in 1 blocks
==90776==    indirectly lost: 159,984 bytes in 9,999 blocks
==90776==      possibly lost: 0 bytes in 0 blocks
==90776==    still reachable: 0 bytes in 0 blocks
==90776==         suppressed: 0 bytes in 0 blocks
==90776== 
==90776== ERROR SUMMARY: 1 errors from 1 contexts (suppressed: 0 from 0)
```
