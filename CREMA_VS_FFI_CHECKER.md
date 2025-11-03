# Crema vs. FFI Checker Performance on Rust–C FFI Test Cases
### **(test cases located in ./tests_and_target_repos/a-code_c_ffi)**

## The experiments were conducted in two separate Docker containers running on a Debian virtual machine with the following specifications:
* Platform: Oracle VirtualBox
* OS name: Debian GNU/Linux 12 (bookworm)
* OS type: 64-bit
* Memory: 6,8 GiB
* Processors: Intel Core i7-9750H x 4


## Docker images:
### Crema: pull and run the docker image
```bash
docker pull andreafranceschi/crema-tool:latest
```
```bash
docker run -it andreafranceschi/crema-tool:latest bash
```

### FFI-Checker: pull and run the docker image
```bash
docker pull andreafranceschi/rust-ffi-checker:latest
```
```bash
docker run -it andreafranceschi/rust-ffi-checker:latest bash
```

## Methodology
* Each target project was compiled (built) once
* Both Crema and FFI Checker were then executed exactly once per project to measure execution time



| **Test name**                | **Crema output**                | **FFI checker output**                          | **Expected output** | **Crema Real Execution** | **FFI Checker Real Execution Time** | 
|------------------------------|---------------------------------|-------------------------------------------------|---------------------|--------------------------|-------------------------------------|
| branch_df_mem_leak_ffi       | DF ✅ - UB/UAF ✅ - MemLeak ✅  | DF ✅ - UB/UAF ✅ - MemLeak ✅                  |DF-UB/UAF-MemLeak    | 2.012s                   | 6.870s               |                
| clean_mul_fn_ffi_no_errors   | No Errors ✅                    | No Errors ✅                                    |No Errors            | 1.712s                   | 6.005s                              |
| cstr_cargo_df_ffi            | DF ✅ - UB/UAF ✅               | DF ✅ - UB/UAF ✅                               |DF-UB/UAF            | 0.691s                   | 7.154s                              |
| cstr_expect_uaf_and_ub_ffi   | UB/UAF ✅                       | DF ❌ (false positive) - UB/UAF ✅              |UB/UAF            | 0.643s                   | 2.027s                              |
| cstringcargo_df_ffi          | DF ✅ - UB/UAF ✅               | DF ✅ - UB/UAF ✅ - MemLeak ❌ (false positive) |DF-UB/UAF            | 2.012s                   | 6.703s                              |
| df_rand_cargo_c_ffi          | DF ✅ - UB/UAF ✅               | DF ✅ - UB/UAF ✅ - MemLeak ❌ (false positive) |DF-UB/UAF            | 1.872s                   | 7.108s                             |
| for_df_ffi                    | DF ✅ - UB/UAF ✅              | DF ✅ - UB/UAF ✅ - MemLeak ❌ (false positive) |DF-UB/UAF               | 1.905s                   | 7.129s                             |
| for_memory_leak_ffi          | MemLeak ✅                      | MemLeak ✅                                      |UB/UAF-MemLeak       | 1.936s                   | 7.208s                             | 
| uaf_mem_leak_ffi             | UB/UAF ✅ - MemLeak ✅          | DF ❌ (false positive) - UB/UAF ✅ - MemLeak ✅ |UB/UAF-MemLeak       | 1.797s                   | 6.932s                             |
| vuln_only_mem_leak_but_df_brach_overapprox_ffi |  DF ❌ (false positive) - UB/UAF ❌ (false positive) - MemLeak ✅ | DF ❌ (false positive) - UB/UAF ❌ (false positive) - MemLeak ✅ |MemLeak | 1.878s                   | 6.907s     |
| warning_ub_bool               | UB/UAF ✅                            | DF ❌ (false positive) - UB/UAF ✅ - MemLeak ❌ (false positive) |UB/UAF | 1.975s   | 6.847s                            |
| warning_ub_int                | UB/UAF ✅                            | DF ❌ (false positive) - UB/UAF ✅ - MemLeak ❌ (false positive) |UB/UAF | 0.710s   | 7.044s                            |
| warning_ub_mult               | UB/UAF ✅                            | DF ❌ (false positive) - UB/UAF ✅ - MemLeak ❌ (false positive) |UB/UAF | 1.675s   | 6.950s                            |
| warning_ub_string             | UB/UAF ✅                            | DF ❌ (false positive) - UB/UAF ✅ - MemLeak ❌ (false positive) |UB/UAF| 1.665s   | 6.522s                            |

