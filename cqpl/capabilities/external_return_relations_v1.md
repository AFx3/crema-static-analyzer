# external_return_relations_v1 (ERR1)

Additive schema-v2 capability requiring `mir_semantics_v2`. EFM1 and EFM2 remain unchanged. The producer admits only exact selected-crate foreign symbols, exact arities, and calls with no represented body. Each record carries the real MIR call, result local, and (where applicable) formal-0 actual. Independent `external_return_call_bindings` preserve the MIR destination and argument identities; the consumer validates both surfaces atomically.

| symbol/arity | relation | nullability | ownership |
|---|---|---|---|
| memcpy/3 | exact_argument_alias(0) | same_as_source | alias_existing |
| memmove/3 | exact_argument_alias(0) | same_as_source | alias_existing |
| memset/3 | exact_argument_alias(0) | same_as_source | alias_existing |
| memchr/3 | nullable_derived_alias(0) | nullable | alias_existing |
| strchr/2 | nullable_derived_alias(0) | nullable | alias_existing |
| getenv/1 | nullable_borrowed_external | nullable | borrowed_external |

Exact aliases copy the source's candidate identities and provenance without upgrading access-only provenance. Derived aliases populate the separate `access_bases` identity component: this supports ordinary MAY memory-use events but is excluded from deallocation identity resolution. Pointer copies, casts and ownership transfers preserve this distinction. Existing summarized pointer offsets conservatively produce access-only associations, never base-free certificates. Unknown sources remain unknown; multiple candidates remain MAY candidates.

ERR1 adds no allocations, allocator families, or leak obligations. `getenv` has no source alias and no caller-owned allocation; later environment mutation invalidation is not modeled. Exact zero-byte memcpy/memmove/memset retain return equality independently of EFM2 memory-effect suppression. Exact zero-byte memchr has no positive derived association. Dynamic lengths retain the nullable MAY association.

Every record uses `crema_err1_closed_contract_v1`; each closed tuple's semantic class and POSIX provenance token are enforced independently by the checker and JSON schema. Unknown/duplicate tuples, undeclared or cross-scope locals, mismatched call destinations/actuals, represented bodies, fresh allocations, and derived/borrowed base certificates fail closed. No CQPL truth, RN1, AGE1, RBF or CR semantics are changed.

The normative implementation and acceptance specification is [D2_EXTERNAL_RETURN_RELATIONS_V1_GATE.md](../D2_EXTERNAL_RETURN_RELATIONS_V1_GATE.md). ERR1 is not a general pointer-provenance or deallocation-validity analysis.
