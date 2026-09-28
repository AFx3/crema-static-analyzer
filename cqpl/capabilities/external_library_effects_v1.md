# External library effects V1 (ELE1)

ELE1 is consolidation metadata. Existing allocation, realloc, deallocation,
formal-memory, return-relation, and negative-evidence records remain the sole
semantic sources. ELE1 creates no allocation, drop, read, write, realloc,
return-alias, liveness, escape, family, or identity fact.

An effectful bodyless Rust MIR call has exactly one `external_call_bindings`
record and one `external_library_effects` envelope. The binding ID is
`ele1:` followed by the call node. Its closed basis is
`rustc_mir_external_call_binding_v1`. Arguments are ordered MIR actuals, with
null where the existing variable catalog has no canonical Rust variable.
The result is likewise nullable. ELE1 does not expand the variable catalog.
Represented-body calls have no ELE1 binding or envelope.

The envelope has basis `crema_external_library_effects_v1` and exact counts for
six families: `allocation_return`, `reallocation`, `deallocation`,
`formal_memory`, `return_relation`, and `negative_evidence`. Counts refer to
validated underlying records. Allocation returns use existing callsite-local
`c_call` allocation records and allocator contracts; realloc uses RBF records;
deallocation uses D4-P0 call provenance; formal memory uses EFM2; returns use
ERR1; negative evidence uses ENE1. CR1 corroboration and allocation/drop labels
do not add a second direct-free count. No family is inferred from symbol-name
matching in the checker. Unresolved effectless calls have no envelope.

In ELE1 artifacts, `external_call_bindings` is the sole serialized call-binding
authority. Frozen ERR1 artifacts without ELE1 retain the legacy
`external_return_call_bindings` protocol. The checker validates internal
artifact consistency; producer tests and the D4 gate establish real MIR
origin. The protocol makes no cryptographic tamper-resistance claim.

The accepted semantic rules remain those of
[EFM2](external_formal_memory_effects_v2.md),
[ERR1](external_return_relations_v1.md),
[ENE1](external_negative_evidence_v1.md), and
[D4-P0](external_deallocation_call_provenance_v1.md).
