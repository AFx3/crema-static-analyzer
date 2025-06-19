## TOT CARGO PROJECTS: 73
## TOT VULN: 68
## FALSE POSITIVES: 2
## FALSE NEGATIVES: 0
## TRUE POSITIVE: 68
## TRUE NEGATIVE: 10 (DI CUI 3 IN 1 in warning_ub_mult)

### Precision: crema generates a FP in ~2,86% of the cases in which in rises up a vulnerability
$\displaystyle \frac{TP}{TP + FP}$

$\frac{68}{68 + 2} = \frac{68}{70}$ 
= 0,9714 (97,14%)

### Recall: crema has found all the vulnerabilities (no FN).

$\displaystyle \frac{TP}{TP + FN}$

$\frac{68}{68 + 0} = \frac{68}{68}$
= 1,000 (100 %)

### Accuracy: crema correctly classifies 97,5 % overall cases.


$\displaystyle \frac{TP + TN}{TP + TN + FP + FN}$

$\frac{68 + 10}{68 + 10 + 2 + 0} = \frac{78}{80}$
= 0,975 (97,5 %)

### F1-score: balances precisions and recall 

$2\cdot\frac{\text{Precisione}\cdot\text{Recall}}{\text{Precisione} + \text{Recall}}$

$2\cdot\frac{0,9714\cdot1}{0,9714 + 1} \approx 0,9855$
= 0,9855 (98,55 %)

### Specificity: the tool correctly identifies 83.33% of non-vulnerable cases

$\displaystyle \frac{TN}{TN + FP}$

$\frac{10}{10 + 2} = \frac{10}{12}$
= 0,8333 (83,33 %)

### False Positive Rate: the tool flags a vulnerability in 16.67% of non-vulnerable cases

$\displaystyle \frac{FP}{FP + TN}$

$\frac{2}{2 + 10} = \frac{2}{12}$
= 0,1667 (16,67 %)


## Cargo projects Rust only: 
* mem-leak (literals) 
    * 16 ✅
* df (literals)
    * 16 ✅
* uaf (literals)
    * 16 ✅


* **clean_cstring_no_errors_only_rust** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **clean_into_from_raw** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **clean_struct_point_mem_leak_into_raw_no_errors** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **closure_df** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_full_rust/closure_df/src/main.rs:35:9: 35:33 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **cstring_df_only_rust** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_full_rust/cstring_df_only_rust/src/main.rs:21:9: 21:25 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```


* **cstringcargo_enum_df_only_rust** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs:33:50: 33:51 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```


* **explicit_drop_df_only_rust** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_full_rust/explicit_drop_df_only_rust/src/main.rs:7:9: 7:33 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **std_mem_forget_mem_leak_rust** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_2)}": 1}

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **struct_point_df_only_rust** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_full_rust/struct_point_df_only_rust/src/main.rs:44:43: 44:44 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **struct_point_mem_leak** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{Local(_5)}": 1}

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **uaf_read_ptr** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_full_rust/uaf_read_ptr/src/main.rs:11:38: 11:41 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

## Cargo projects Rust + C FFI

* **branch_df_mem_leak_ffi** ✅ ✅ (x2)
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
☢ Never Free Issues ☢:
{"{32@rust::main::bb20, Local(_31)}": 1}
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '10@rust::main::bb3' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **clean_mul_fn_ffi_no_errors** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **cstr_cargo_df_ffi** ✅ ✅ DA VEDERE IL CONTEGGIO (x2) 🚨 🚨
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_c_ffi/cstr_cargo_df_ffi/src/main.rs:43:35: 43:38 (#0)
{"{10@rust::main::bb9, Local(_16), Local(_4), Local(_6), _16}": 1}
☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode9 {fun: print_e_free}
   call void @free(ptr noundef %1) CallICFGNode: 
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '10@rust::main::bb9' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode9 {fun: print_e_free}
   call void @free(ptr noundef %1) CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```
*DA FAR VEDERE A LILLO per classificare*:
```
let rust_str = CStr::from_ptr(ptr).to_string_lossy(); MAY LEAD TO UB

```

* **cstr_expect_uaf_and_ub_ffi** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_c_ffi/cstr_expect_uaf_and_ub_ffi/src/main.rs:19:35: 19:38 (#0)
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '10@rust::main::bb9' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode9 {fun: print_e_free}
   call void @free(ptr noundef %1) CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **cstringcargo_df_ffi** ✅ 
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode17 {fun: modify_and_free_string}
   call void @free(ptr noundef %3) #3 CallICFGNode: 
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '11@rust::main::bb6' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode17 {fun: modify_and_free_string}
   call void @free(ptr noundef %3) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

```


* **df_rand_cargo_c_ffi** ✅  🚨 
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_c_ffi/df_rand_cargo_c_ffi/src/main.rs:26:40: 26:57 (#0)
{"{14@rust::main::bb2, Local(_19), Local(_2), Local(_20), Local(_20) [mutable], _19}": 1}
☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode21 {fun: cast}
   call void @free(ptr noundef %6) #3 CallICFGNode: 
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '14@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode21 {fun: cast}
   call void @free(ptr noundef %6) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **for_df_ffi** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode13 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '10@rust::main::bb11' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode13 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **for_memory_leak_ffi** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Never Free Issues ☢: {"{32@rust::main::bb11, Local(_20)}": 1}

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
```

* **uaf_mem_leak_ffi** ✅ ✅ (x2)
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Use-After-Free Issues / Undefined behaviour ☢:
Use detected at source line: /home/af/Documenti/a-phd/cargo_project_test/a-code_c_ffi/uaf_mem_leak_ffi/src/main.rs:11:57: 11:78 (#0)
{"{47@rust::main::bb5, Local(_12), Local(_14), Local(_2)}": 1}
☢ Never Free Issues ☢:
{"{Local(_23)}": 1}
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '47@rust::main::bb5' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode44 {fun: free_int}
   call void @free(ptr noundef %5) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

```

* **vuln_only_mem_leak_but_df_branch_overapprox_FFI** ✅ 🚨 (OK errore CONTROLLATO)
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 Potential memory issues detected 🚀:

☢ Double Free Issues ☢:
Free detected at source line: CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: 
☢ Never Free Issues ☢:
{"{32@rust::main::bb20, Local(_31)}": 1}
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '10@rust::main::bb3' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode11 {fun: print_and}
   call void @free(ptr noundef %1) #3 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **warning_ub_bool** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '29@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode28 {fun: free_bool}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **warning_ub_int** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '45@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode38 {fun: free_int}
   call void @free(ptr noundef %3) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```

* **warning_ub_mult** ✅ ✅ ✅ (x3)
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '9@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode18 {fun: free_bool}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
🪲  WARNING: variable '60@rust::main::bb5' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode50 {fun: free_int}
   call void @free(ptr noundef %3) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
🪲  WARNING: variable '29@rust::main::bb9' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode28 {fun: free_str}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!

```

* **warning_ub_string** ✅
```
⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🤖💬 NO Issues detected: ✅

⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦⬦
🪲  WARNING: variable '29@rust::main::bb2' was allocated in Rust and then freed in C (LLVM free) at `CallICFGNode28 {fun: free_str}
   call void @free(ptr noundef %1) #2 CallICFGNode: `
Possible UNDEFINED BEHAVIOUR!
```