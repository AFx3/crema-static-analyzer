# Regression-test inventory

Prepared but not executed.

## Newly added in this port

| Area | Test | Assertion |
|---|---|---|
| Closure | `p3_r1_direct_closure_call_propagates_captured_allocation_identity` | A certified direct `rustc_closure_body_v1` environment carries the captured `AbstractAllocId` to formal local 1. |
| Closure negative | `p3_r1_uncertified_closure_boundary_remains_fail_closed` | Indirect calls and unsupported ABIs do not import allocation identity. |
| Drop/UAF negative | `drop_in_place_is_not_exported_as_a_uaf_write_event` | Drop glue is not a synthetic write; a real `ptr::write` remains a write event. |
| Box drop | `certified_box_drop_glue_pointer_extraction_is_not_a_uaf_read` | Typed Box payload extraction is not a user freed-memory read; an ordinary dereference remains observable. |
| Destructor receiver | `certified_destructor_receiver_reborrow_is_not_a_uaf_read` | Certified `Drop::drop` receiver reborrow is suppressed without suppressing ordinary reborrows. |
| Constant false | `certified_constant_false_exports_only_false_successor` | A typed certificate exports exactly the false target. |
| Constant true | `certified_constant_true_exports_only_true_successor` | A typed certificate exports exactly the true target. |
| Dynamic branch | `dynamic_switch_preserves_every_successor` | No certificate preserves all targets and the otherwise edge. |
| Bodyless callback | `bodyless_constructor_summary_requires_capability_and_typed_ctor` | The bodyless summary requires both the explicit capability and typed `DefKind::Ctor`; an ordinary missing callback body is not promoted. |

## Existing CQPL8 tests deliberately preserved

- `represented_c_malloc_and_calloc_returned_to_rust_keep_exact_leak_identity`
- `represented_c_allocation_with_reachable_free_has_no_positive_leak_finding`
- `zero_and_unknown_size_realloc_boundaries_remain_fail_closed`
- bodyless `realloc(NULL,n)` and immediate-overwrite certificate tests
- HT2 tests including
  `ht2_r1_actual_formal_preserves_global_heap_store_without_local_environment_copy`
  and the weak-update/no-invented-shape controls.

## End-to-end fixtures in the manual smoke gate

The manual protocol checks `closure_df`, a real UAF, a non-closure real DF,
the recovered `malloc` and `calloc` leaks, `b20b` (ML and DF), `b39`, and the
negative `b20c` realloc control. These fixture executions were not run during
preparation.
