# bmulti_df_second_ffi

Experimental Bmulti fixture, outside frozen FINAL112.

Primary oracle:
- actual[1] / formal[1] -> B
- C free(second) is first deallocation of B
- Box::from_raw(b) followed by Drop is a second deallocation of B
- actual[0] / A must not enter the repeated-drop witness
- A is reclaimed exactly once by Rust
- allocator mismatch evidence is expected on B
- double-free truth remains MAY/unknown, not MUST/true
