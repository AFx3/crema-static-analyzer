# allocation_existence_guards_v1

Status: additive diagnostic evidence capability.

This capability certifies a narrow correlation that is otherwise lost by the
nullable `TOP` abstraction: for one bodyless malloc-family allocation identity
(`malloc`, `calloc`, or producer-certified RN1 `realloc(NULL,n)`),
an exact Rust raw-pointer `is_null` predicate tests the Rust return local of the
same allocation-producing callsite.

It does **not** change CQPL truth, the Kripke transition relation, or CREMA's
abstract lattice. It is consumed only by the UNKNOWN assessment layer.

Each record contains:

- `allocation`: existing top-level `AbstractAllocId`;
- `producer_call_node`: Rust callsite whose return local receives that allocation;
- `predicate_call_node`: exact raw-pointer `is_null` call;
- `switch_node`: immediate boolean `SwitchInt` continuation;
- `tested_variable`: singleton ProgramVarId tested by `is_null`;
- `predicate_result_variable`: boolean MIR return local;
- `null_successor` / `non_null_successor`: exact canonical CFG successors;
- `callee_def_path`: exact rustc DefPath for raw-pointer `is_null`;
- `allocation_return_basis`: one of `rust_foreign_decl_c_malloc_contract_v1` or `svf_single_source_c_allocator_return_v1`;
- `basis = rust_raw_pointer_is_null_switch_v1`.

Producer closure conditions:

1. tested variable has singleton identity `{a#}` at the predicate call;
2. `a#` is a `c_call` site classified exactly as `malloc`, `calloc`, or bodyless
   `realloc`; a `realloc` site can exist in this identity vocabulary only after
   the producer's independent MUST-null proof for formal 0; represented-C
   realloc does not enter this AGE1 extension;
3. the unique MIR call producing the tested variable is the same Rust callsite
   encoded by the allocation site; direct bodyless allocators use the closed foreign-declaration contract, while represented C wrappers additionally require a unique SVF return bridge with a single-source chain from the allocator result; multi-input Phi/Select, multiple definitions, load/store-mediated return flow, or other ambiguous flow fail closed;
4. predicate callee is structurally a raw-pointer `is_null` DefPath;
5. the predicate return target is an exact boolean `SwitchInt` whose false
   successor is non-null and true successor is null;
6. all referenced nodes/variables/allocations are existing producer objects.

The producer intentionally emits no record for a `realloc` result that happens
to carry the old allocation identity: its producer callsite does not match the
original malloc/calloc site. Thus `realloc(...) == NULL` is never interpreted as
non-existence of the old allocation.

Assessment rule:

For an already-UNKNOWN leak query, the null branch may be excluded from the
allocation-obligation proof only when this capability certifies it. The
non-null branch must still be structurally closed by a compatible,
producer-certified deallocation. Mismatched deallocation, unresolved effects,
true escape, site reuse, or unreclaimed-obligation disposition fail closed.
