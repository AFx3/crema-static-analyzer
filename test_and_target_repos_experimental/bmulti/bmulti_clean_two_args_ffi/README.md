# bmulti_clean_two_args_ffi

Experimental negative control, outside frozen FINAL112.

Primary oracle:
- actual[0] / formal[0] -> A
- actual[1] / formal[1] -> B
- C accesses only B and performs no deallocation
- A and B are each reclaimed exactly once via Box::from_raw
- no allocator-family-mismatch finding
- no repeated-drop finding
- no drop-then-use-after-free finding
