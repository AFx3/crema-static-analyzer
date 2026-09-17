# bmulti_uaf_second_ffi

Experimental Bmulti fixture, outside frozen FINAL112.

Primary oracle:
- actual[0] / formal[0] -> Rust allocation A
- actual[1] / formal[1] -> Rust allocation B
- C free(second) deallocates B only
- subsequent Rust dereference uses B after the C deallocation
- A is reclaimed with Box::from_raw
- allocator mismatch evidence is expected on B because the abstract
  contract distinguishes rust_global from c_malloc
- UAF truth should remain MAY/unknown rather than being promoted to MUST/true
