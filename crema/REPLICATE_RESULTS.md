# Instructions for replicating results reported Table 3 and Table 4 of the paper
### scripts:
* tests_and_target_repos/cargo_build_all_dirs.sh 
* run_tests_literals.sh 
* run_tests_mix_only_rust.sh
* run_tests_rust_FFI.sh
* run_github_target_repositories.sh
## Requirements
Follow the instruction in the README.md file to build crema, svf-driver and SVF.

### Step 0: Build test and GitHub target repositories (FROM crema-rust-static-analyzer main folder)

```
From the this directory (crema)
```bash
chmod +x ./tests_and_target_repos/cargo_build_all_dirs.sh
./tests_and_target_repos/cargo_build_all_dirs.sh 
```

### TEST Cargo projects (Paper Table 3)

### Step 1: run crema on only Rust code concerning **doube-frees**, **memory-leaks**, **use-after-frees** on literals
```bash
cd crema
```
```bash
chmod +x ./run_tests_literals.sh  
./run_tests_literals.sh 
```
### Ouput: report also saved in the file z-DF_errors.log, z-ML_errors.log, z-UAF_errors.log
### NOTE: full outputs are truncated for better visibility
* DOUBLE FREES
```bash
=== boxed_bool ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_bool/src/main.rs:13:1: 13:2 (#0)

=== boxed_char ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_char/src/main.rs:13:1: 13:2 (#0)

=== boxed_f32 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_f32/src/main.rs:13:1: 13:2 (#0)

=== boxed_f64 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_f64/src/main.rs:13:1: 13:2 (#0)

=== boxed_i128 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_i128/src/main.rs:13:1: 13:2 (#0)

=== boxed_i16 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_i16/src/main.rs:17:1: 17:2 (#0)

=== boxed_i32 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_i32/src/main.rs:12:1: 12:2 (#0)

=== boxed_i64 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_i64/src/main.rs:11:1: 11:2 (#0)

=== boxed_i8 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_i8/src/main.rs:12:1: 12:2 (#0)

=== boxed_isize ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_isize/src/main.rs:13:1: 13:2 (#0)

=== boxed_u128 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_u128/src/main.rs:13:1: 13:2 (#0)

=== boxed_u16 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_u16/src/main.rs:13:1: 13:2 (#0)

=== boxed_u32 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_u32/src/main.rs:14:1: 14:2 (#0)

=== boxed_u64 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_u64/src/main.rs:14:1: 14:2 (#0)

=== boxed_u8 ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_u8/src/main.rs:11:1: 11:2 (#0)

=== boxed_usize ===
🤖💬 Potential memory issues detected 🚀:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_usize/src/main.rs:14:1: 14:2 (#0) 
```
* MEMORY LEAKS
```bash
=== boxed_bool ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_char ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_f32 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_f64 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_i128 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_i16 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_i32 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_i64 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_i8 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_isize ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_u128 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_u16 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_u32 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_u64 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_u8 ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

=== boxed_usize ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_9)}": 1}

```
* USE-AFTER-FREES
```bash
=== boxed_bool ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_bool/src/main.rs:11:23: 11:27 (#0)

=== boxed_char ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_char/src/main.rs:13:23: 13:27 (#0)

=== boxed_f32 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_f32/src/main.rs:13:23: 13:27 (#0)

=== boxed_f64 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_f64/src/main.rs:11:22: 11:26 (#0)

=== boxed_i128 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_i128/src/main.rs:12:22: 12:26 (#0)

=== boxed_i16 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_i16/src/main.rs:14:24: 14:28 (#0)

=== boxed_i32 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_i32/src/main.rs:12:22: 12:26 (#0)

=== boxed_i64 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_i64/src/main.rs:11:22: 11:26 (#0)

=== boxed_i8 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_i8/src/main.rs:12:24: 12:28 (#0)

=== boxed_isize ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_isize/src/main.rs:13:23: 13:27 (#0)

=== boxed_u128 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_u128/src/main.rs:14:24: 14:28 (#0)

=== boxed_u16 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_u16/src/main.rs:13:24: 13:28 (#0)

=== boxed_u32 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_u32/src/main.rs:12:24: 12:28 (#0)

=== boxed_u64 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_u64/src/main.rs:12:24: 12:28 (#0)

=== boxed_u8 ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_u8/src/main.rs:12:24: 12:28 (#0)

=== boxed_usize ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals/boxed_usize/src/main.rs:12:24: 12:28 (#0)

```



### Step 2: run crema on only Rust code including different memory errors and error-free
```bash
chmod +x run_tests_mix_only_rust.sh   
./run_tests_mix_only_rust.sh
```
### Ouput also saved in Z-only_rust_mix.log
```bash
=== clean_cstring_no_errors_only_rust ===
🤖💬 NO Issues detected: ✅

=== cstringcargo_enum_df_only_rust ===
🤖💬 Potential memory issues detected 🚀:
☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs:33:50: 33:51 (#0)

=== struct_point_df_only_rust ===
🤖💬 Potential memory issues detected 🚀:
☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/struct_point_df_only_rust/src/main.rs:44:43: 44:44 (#0)

=== clean_into_from_raw ===
🤖💬 NO Issues detected: ✅

=== cstring_df_only_rust ===
🤖💬 Potential memory issues detected 🚀:
☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/cstring_df_only_rust/src/main.rs:21:9: 21:25 (#0)

=== struct_point_mem_leak ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_5)}": 1}

=== clean_struct_point_mem_leak_into_raw_no_errors ===
🤖💬 NO Issues detected: ✅

=== explicit_drop_df_only_rust ===
🤖💬 Potential memory issues detected 🚀:
☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/explicit_drop_df_only_rust/src/main.rs:7:9: 7:33 (#0)

=== uaf_read_ptr ===
🤖💬 Potential memory issues detected 🚀:
☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/uaf_read_ptr/src/main.rs:11:38: 11:41 (#0)

=== closure_df ===
🤖💬 Potential memory issues detected 🚀:
☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_full_rust/closure_df/src/main.rs:35:9: 35:33 (#0)

=== std_mem_forget_mem_leak_rust ===
🤖💬 Potential memory issues detected 🚀:
☢ Never Free Issues ☢: {"{Local(_2)}": 1}

```
### Step 3: run crema on Rust code including FFI with different memory errors and error-free
```bash
chmod +x run_tests_rust_FFI.sh
./run_tests_rust_FFI.sh
```
### Ouput also saved in Z-rust_FFI_mix.log
```bash
=== branch_df_mem_leak_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
☢ Never Free Issues ☢:
{"{32@rust::main::bb20, Local(_31)}": 1}
🪲  WARNING: variable '10@rust::main::bb3' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== clean_mul_fn_ffi_no_errors ===
(no memory‑issues detected)

=== cstr_cargo_df_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_ffi/cstr_cargo_df_ffi/src/main.rs:43:35: 43:38 (#0)
{"{10@rust::main::bb9, Local(_16), Local(_4), Local(_6), _16}": 1}
☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode9 {fun: print_e_free}
   call void @free(ptr noundef %1) CallICFGNode: 
🪲  WARNING: variable '10@rust::main::bb9' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode9 {fun: print_e_free}
   call void @free(ptr noundef %1) CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== cstr_expect_uaf_and_ub_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_ffi/cstr_expect_uaf_and_ub_ffi/src/main.rs:19:35: 19:38 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '10@rust::main::bb9' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode9 {fun: print_e_free}
   call void @free(ptr noundef %1) CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== cstringcargo_df_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode17 {fun: modify_and_free_string}
   call void @free(ptr noundef %3) #3 CallICFGNode: 
🪲  WARNING: variable '11@rust::main::bb6' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode17 {fun: modify_and_free_string}
   call void @free(ptr noundef %3) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== df_rand_cargo_c_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_ffi/df_rand_cargo_c_ffi/src/main.rs:26:40: 26:57 (#0)
{"{14@rust::main::bb2, Local(_19), Local(_2), Local(_20), Local(_20) [mutable], _19}": 1}
☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode21 {fun: cast}
   call void @free(ptr noundef %6) #3 CallICFGNode: 
🪲  WARNING: variable '14@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode21 {fun: cast}
   call void @free(ptr noundef %6) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== for_df_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode13 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
🪲  WARNING: variable '10@rust::main::bb11' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode13 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== for_memory_leak_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{32@rust::main::bb11, Local(_20)}": 1}


=== uaf_mem_leak_ffi ===
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_ffi/uaf_mem_leak_ffi/src/main.rs:11:57: 11:78 (#0)
{"{47@rust::main::bb5, Local(_12), Local(_14), Local(_2)}": 1}
☢ Never Free Issues ☢:
{"{Local(_23)}": 1}
🪲  WARNING: variable '47@rust::main::bb5' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode44 {fun: free_int}
   call void @free(ptr noundef %5) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== vuln_only_mem_leak_but_df_branch_overapprox_FFI ===
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
☢ Never Free Issues ☢:
{"{32@rust::main::bb20, Local(_31)}": 1}
🪲  WARNING: variable '10@rust::main::bb3' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== warning_ub_bool ===
🪲  WARNING: variable '29@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode28 {fun: free_bool}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== warning_ub_int ===
🪲  WARNING: variable '45@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode38 {fun: free_int}
   call void @free(ptr noundef %3) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== warning_ub_mult ===
🪲  WARNING: variable '9@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode18 {fun: free_bool}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
🪲  WARNING: variable '60@rust::main::bb5' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode50 {fun: free_int}
   call void @free(ptr noundef %3) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
🪲  WARNING: variable '29@rust::main::bb9' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode28 {fun: free_str}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

=== warning_ub_string ===
🪲  WARNING: variable '29@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode28 {fun: free_str}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

```

-----------------------------------------------------------------------------------------------

### TEST GitHub projects (Paper Table 4)
### Step 4: run crema on GitHub repositories
```bash
chmod +x run_github_target_repositories.sh
./run_github_target_repositories.sh
```
### Ouput also saved in Z-github_target_repos.log
### NB: openapi-client-gen will take a while (big one)
```bash
=== noGenerator ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_8), Local(_9)}": 1, "{Local(_10)}": 1}


=== wasm-demo ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_0)}": 1}


=== skip-list-test ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_33)}": 1, "{Local(_13)}": 1, "{Local(_9)}": 1, "{Local(_37)}": 1, "{Local(_5)}": 1, "{Local(_25)}": 1, "{Local(_29)}": 1, "{Local(_21)}": 1, "{Local(_17)}": 1, "{Local(_1)}": 1}


=== rusant ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_0)}": 1}


=== napkin-math-test ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_18), Local(_4), Local(_7)}": 1}


=== shared-register ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_17)}": 1}


=== whisper-rs-example ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_7)}": 1, "{Local(_6)}": 1}


=== lock-free ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_14), Local(_2), _14}": 1}


=== rust_memory ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_13)}": 1, "{Local(_33)}": 1, "{Local(_34)}": 1, "{Local(_14)}": 1}


=== unsized_struct ===
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_0), Local(_2) [mutable], _0}": 1}


=== unsafely-created-owned-type ===
🤖💬 NO Issues detected: ✅


=== lock_free_non_blocking_linked_list ===
🤖💬 NO Issues detected: ✅


=== c-callback-rust-closure ===
🤖💬 NO Issues detected: ✅


=== concurrent-verification ===
🤖💬 NO Issues detected: ✅


=== stackswap-coroutines ===
🤖💬 NO Issues detected: ✅


=== rc-playground ===
🤖💬 NO Issues detected: ✅


=== square ===
🤖💬 NO Issues detected: ✅


=== rust_hw3 ===
🤖💬 NO Issues detected: ✅


=== openapi-client-gen ===
🤖💬 NO Issues detected: ✅


=== rust-boks ===
🤖💬 NO Issues detected: ✅


```