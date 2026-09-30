# DEP1-P3-RIC1 — Resource identity continuity

**Status:** P3-P0 protocol proposal for independent review. No P3 production implementation or resource-continuity acceptance is claimed.

**Provisional opt-in capability:** `dependency_resource_identity_continuity_v1`.

**Immediate production parent:** `b092bc5014b74939f9acf298658921491726d7f2`, branch `cqpl7-dependency-body-ingestion`. The phase began with a clean worktree. This full commit, including its accepted P2 implementation, is the implementation baseline. `446d67db98b20043ec35d49ca042a1f7c86dc9f5` remains a historical DEP1-disabled comparison, not the immediate P3 parent.

**Compiler:** `nightly-2024-11-21`, rustc `3fee0f12e4f595948f8f54f57c8b7a7a58127124`.

**Evidence:** `repro-results/dep1-p3-p0-ric0-20260930T104539Z/`. Source audit entries carry file, declaration line, and SHA256. Numbered source snapshots preserve the exact inspected text. Accepted P2 evidence remains unchanged at `repro-results/dep1-p2-r5-evc2-20260930T101500Z/`; its 233 manifest entries independently verified, with zero missing or mismatched entries.

## 1. Scope and semantic ownership

P2 establishes the interprocedural execution path. P3 must establish that the same tracked abstract resource crosses a represented Rust call boundary without accidental capture, splitting, or fabricated allocation:

```text
caller actual -> callee formal -> body operation/effect
              -> normal return / caller-visible effect -> caller destination
```

This protocol concerns existing supported abstract allocation/value semantics. It does not authorize a whole-program pointer analysis, dynamic allocation-instance tracker, heap-shape domain, path-sensitive ownership verifier, recursive resource solver, or a new CQPL resource resolver.

COV1 remains authoritative. Only an exact represented Rust activation with accepted Rust ownership may receive P3 Rust bindings. Local represented Rust calls use the same boundary discipline where needed to preserve nested dependency flow; this does not enlarge the accepted dependency runtime scope.

| Existing owner | P3 rule |
|---|---|
| `dep1_rust_mir` | Bind only to the exact represented Rust Instance and P2 activation. |
| `crema_ffi_body` | Preserve the existing imported C/LLVM and Rust/SVF resource bridge. |
| `cqpl_external_library_model` | Preserve the existing exact ELE1 binding/effect envelope and its resource semantics. |
| `uncovered` / unavailable or unresolved body | No fabricated Rust body or resource binding; retain explicit incompleteness where required. |

A foreign declaration, ABI, or function spelling is not a semantic-coverage certificate. `dep::abs` has no special rule. Recompute its exact owner from producer artifacts and retain that result. This protocol does not infer delegation from its historical body-unavailable record.

## 2. Prerequisite P2 contract

P3 consumes the accepted authoritative context-explicit CREMA execution graph `G` and its exact annotated export `A`. It inherits native compiler identities, call keys, concrete dispatch, normal return targets, unwind pairing, nested contexts, diverging-call absence of normal return, and reachable-recursion fail-closed behavior.

Canonical body identity is the accepted concrete rustc Instance ID. Canonical code identity is `(Instance, MIR BasicBlock)`. Execution identity adds the exact ordered static `call_key` stack. Root context is `[]`; a call `K` pushes it to `C ++ [K]` and its matched normal/unwind pop returns to `C`.

P3 must not resolve callees again. No matching by pretty function/crate name, source path, textual node prefix, or argument arity alone is allowed. Existing compatibility adapters may produce String keys; their identity must be certified through the structured P2 sidecar, not inferred by an independent name resolver.

CQPL may retain its accepted entry projection, conditional-reallocation semantic elaboration, and truth/quiescence completion. These checker states are not Rust call/return transitions or allocation/return-resource producers.

## 3. Current architecture: measured source facts

The following are source facts at the immediate parent, not newly measured P3 dynamic results.

| Concept | Actual representation | Boundary |
|---|---|---|
| Program variable | `ProgramVarId::Rust { function, local }`, C `{ function, var_id, callsite }`, or Synthetic | P2 rewrites Rust function scope to its deterministic execution scope. Legacy memory names remain separately unscoped. |
| Allocation identity | `AbstractAllocId { site: AllocationSiteId, context: Vec<String> }` | A finite context-qualified static allocation site, not a dynamic object. |
| Resource identity | No separate `ResourceId` type | In this protocol, a tracked resource is an existing `AbstractAllocId`. |
| Alias relation | `AllocationIdentityMemory.points_to`, `stack_refs`, `access_bases`, projected maps | MAY candidates/reference relations. MAY overlap is not transitive equivalence. |
| Place | `PlaceId { base: ProgramVarId, projection }` | `Deref`, `Field`, `Index`, `Opaque` variants exist; extraction/transfer support is partial. |
| Liveness/deallocation | `CellValue` plus lifecycle/disposition evidence | No first-class live/freed/invalid/escaped state machine in the identity domain. |
| Actual | `MirCallArgument { arg: String, is_mutable }` in `RustCallMetadata.arguments` | Not a structural typed value identity. |
| Formal | `RustFunctionMetadata.arg_count`; MIR locals `_1.._arg_count` | Count/position alone does not certify ABI/operand shape. |
| Return value | MIR `_0`, `RustCallMetadata.return_place` and exact P2 continuation | No first-class `ReturnValueId`. |

`AllocationIdentityMemory` stores heap candidates separately from stack-place references. Taking `&x` denotes a place; it does not automatically denote the heap allocation that `x` contains. `allocations_for_place` follows existing finite reference/place evidence. `event_allocations` may include access-only bases; `deallocation_allocations` deliberately excludes that access-only association as a base-free certificate.

`copy_binding` preserves points-to candidates, stack references, access bases, and represented projected fields. The fixed point has node/context post snapshots and intra-node event summaries. Event summaries union identities around statement boundaries so a later overwrite cannot erase an earlier event subject. They do not establish arbitrary event ordering or path sensitivity.

The legacy domain is different: `Allocation` is a set of `Name = String` aliases; `AbstractMemory` maps those components to `CellValue`. Its lattice is `BOTTOM`, `BOXTIMES`, `ALLOC`, `FREED`, `MB`, `IMMB`, `MV`, `TOP`. In particular `ALLOC ⊔ FREED = TOP`. `MV` is the existing ownership-forgotten/raw convention, not a universal escaped-resource theorem. `return_escape` disposition is MAY provenance, not proof that deallocation obligations have been discharged.

`LifecycleValue` is a separate OR-joined MAY satellite: `may_own`, `may_partial_drop`, `may_stale_owner`, `may_committed`, `may_complete`. It does not replace `CellValue` or introduce concrete owning-object identity.

The exporter serializes the whole `AbstractAllocId` as the opaque allocation ID. `allocation_post` currently lifts legacy state through direct points-to identities and joins contributions. It intentionally does not lift reference-temporary state into its pointee by following `stack_refs`. CQPL consumes these existing authoritative fields; the projection is not proof that a dependency effect was correctly propagated before export.

## 4. Existing support and gaps

Current local actual/formal and `_0` return helpers already preserve allocation candidates on supported inputs. Source tests include `phase6c_actual_formal_and_return_preserve_allocation_id`, context-qualified allocation-site distinction, reference/place flows, and positional C bindings. P0 inspected these tests but did not rerun them or treat them as cross-crate P3 acceptance.

The audit identifies requirements for subsequent implementation:

1. `bind_internal_actuals` still uses unscoped legacy `Local(_i)` names. P2 execution nodes alone do not prevent actual/formal local-index collisions in `AbstractMemory`.
2. `convert_terminator` coalesces compiler `Operand::Copy` and `Operand::Move` into the same `MirCallArgument` description. Its `is_mutable` records the local declaration's mutability, not a shared/mutable-reference or ABI certificate.
3. Existing identity actual selection uses a local-token helper; that is insufficient to certify an arbitrary projected actual. The actual field/referent must not be replaced with the first base-local candidate.
4. `bind_return_to_caller` joins the caller snapshot with callee memory, then copies `_0`. This is an existing MAY join, not a theorem of exact caller-visible state-changing effects. Stale candidates and snapshot joins must be accounted for explicitly.
5. Represented diverging calls may have a P2 activation but no normal-return `RustCallMetadata`. Actual/formal support must use that exact activation and compiler arguments, not require a fictional return relation.
6. The identity profiles `LegacyFrozen` and `DispositionV6S` intentionally differ on projected destination treatment. P3 must not silently switch or merge them.
7. P2 emits `DEP1 unwind exit -> matched caller cleanup`. The current `panic_unwind::edge_flow_kind` vocabulary does not list it and defaults unmatched labels to Normal. P2 control matching remains accepted; exact resource unwind propagation requires a P3 edge-aware proof using its accepted relation. P0 makes no production correction.

These are concrete implementation requirements and fail-closed boundaries. They do not make the existing abstract lattice unusable, and they do not authorize export-only repairs or unreviewed abstraction changes.

## 5. Allocation identity layering decision

**Preserve the current context-qualified static allocation-site abstraction.** `CanonicalResourceIdentity` means the whole existing `AbstractAllocId`, including its existing context; canonical does not mean context-free.

The allocation-analysis context is currently `bounded_context(call.call_node) = [call.call_node]`. This is not the P2 call-key stack. Under P2, `call_node` is already qualified by its native Instance and exact caller execution context. Rust allocator transfer records the current execution node as `AllocationSiteId::RustCall.node_id` and the current analysis context in `AbstractAllocId.context`.

Therefore two executions of one canonical allocation code site under different P2 activations currently produce distinct abstract site identities. Conversely, repeated runtime visits to the same abstract site/context, including a CFG loop, merge into the same abstract resource. This is context-sensitive allocation-site abstraction, not a new object counter or dynamic allocation tracker.

P3 must preserve that rule. It must neither erase the execution-qualified site/context nor replace the bounded analysis context with a new full-stack allocation policy. A resource passed from caller to callee retains its original whole `AbstractAllocId`; the callee must not recontextualize it merely because its formal lives in another activation.

`ResourceExecutionProvenance` separately records where a relation is observed: call key, caller/callee Instances, ordered contexts, value/place, and exact P2 transition. An allocation origin proof links the existing resource site to canonical `(Instance, BB)` through the P2 state table. An ordinal is required only if an existing supported producer can emit distinct allocations within that same code site; no current multiplicity is invented.

Names retained in existing allocation summaries are diagnostic/classifier metadata. Origin joins must use the exact existing resource value and compiler/P2 provenance. Generic/trait/version collisions cannot be resolved by those names.

## 6. Formal activation and resource relation

Let an accepted P2 activation be:

```text
a = (K, I_caller, I_callee, C, C++[K], call_state,
     callee_entry, normal_return_relations, unwind_action/cleanup)
```

Its identity is `(K, C)` with the Instances/endpoints certified by P2. Static `K` alone is not a nested execution activation. All P3 relations refer to this object; they must not create graph transitions.

For identity memory `M`, write `Pts_M(v)`, `Refs_M(v)`, `Access_M(v)` and the represented projected subtree for a value/place `v`. These are finite MAY relations, not concrete addresses. A resource-binding observation is:

```text
B(a, relation_kind, caller_place, callee_place,
  candidate_AbstractAllocId_set, evidence_boundary, status)
```

The set can be empty, singleton, or multiple. `resource_binding_available` certifies a supported abstract transfer; it is not a concrete MUST-alias assertion. A resource-relevant unresolved empty set is not silently `no_tracked_resource`. The latter requires positive compiler/domain evidence that the value is nonresource.

Exact pass-through/alias transfer preserves the justified candidate IDs. Where the source has a singleton `R`, the supported target must have that same `R` and no fabricated fresh ID. Where a source is a MAY set, the transfer must preserve the applicable set and certainty, not select a convenient singleton or transitively close MAY overlap.

## 7. Actual to formal

For certified operand `actual_i` and exact instantiated MIR formal `_i+1` of activation `a`, transfer supported identity/reference/access and projected-place relations into the callee-scoped formal. Preserve the IDs, not the variable name. Verify compiler argument count, hidden/closure environment conventions, and formal layout before declaring positional equality.

| Shape | Existing basis | P3 acceptance requirement |
|---|---|---|
| Raw pointer local | Direct points-to and selected copy/cast summaries | Structural operand/place; preserve base vs access-only distinction. |
| Shared reference | Stack-place refs and selected dereference transfer | Preserve exact referent; reference state is not pointee state. |
| Mutable reference | Same reference/place relations | Supported indirect write updates the certified referent/resource; unknown shape is incomplete. |
| Box/owning representation | Existing allocation/conversion/drop models | Demonstrated modeled layout only; no general ADT or ownership claim. |
| Aggregate field | Projected maps, capture/downcast rebasing | Exact field projection, not base-local fallback. Unknown indexing/subslice is unsupported. |
| Move | Existing identity-preserving assignment | Capture Copy/Move discriminant; resource continuity separate from supported move-state effects. |
| Copy | Existing identity/reference copy | Same justified candidates; no fresh formal allocation. |
| Same actual to multiple formals | Positional binding | Same resource stays shared; no forced splitting or MAY-equivalence closure. |

Current compiler extraction has the typed operands and places while `TyCtxt` is available. The minimum future addition is a supported structural operand/place certificate alongside the existing adapter, reusing `ProgramVarId`/`PlaceId` shapes. Do not serialize session-local `DefId`/`CrateNum` as persistent IDs or implement a custom GenericArgs identity serializer.

Constants, unsupported projection kinds, unresolved layouts, and arbitrary heap-indirect paths require specific statuses. Scalar/nonresource constants may be explicitly not applicable. A tracked pointer cannot be declared resolved merely because its text contains a local token.

## 8. Return to exact caller destination

At a matched normal return only, read the identity of `_0` in the exact callee activation and bind it to the compiler-certified destination of the same P2 pop. Never infer destination from block order, name, or another call to that body.

Distinguish:

- **Exact input alias:** retain the original input resource candidate IDs. Do not synthesize a return allocation.
- **Supported derived alias:** preserve only the existing derived/access association; it does not certify base deallocation or full ownership.
- **Callee-created allocation:** retain that existing modeled allocation's full site/context and compiler-qualified origin. Do not assign an input or caller allocation origin.
- **Proven no tracked resource:** explicitly classify nonresource value; no fabricated identity.
- **Unresolved return:** retain `unresolved_return` or the more specific identity/shape reason and mark required continuity incomplete.

The existing `_0` copy helper is reusable only with these activation, shape, and status checks. Supported projected destinations must use a certified place. Unsupported ones cannot silently bind the base local.

## 9. Side effects on the same resource

Resource identity memory alone has no freed/live bit. The side-effect gate must coordinate it with `AbstractMemory`, taint, disposition, and lifecycle transfers without changing their accepted lattice/meaning.

If caller knows `R`, a supported callee free/drop/invalidation/read/write operation resolves to `R` through formal/referent evidence. The caller subsequently observes the state/event of that same `R`. Creating a new formal-local `R'` and freeing it is forbidden.

A write to an aggregate referent may affect a caller place; a write to a formal pointer slot need not mutate its caller variable. Do not wholesale copy all formal locals back to actual locals. Use the supported exact referent/side-effect relation. Arbitrary heap shape and indirect writes remain unsupported.

The final producer state, not just a ledger, must show the effect. Existing control/dataflow joins may widen `ALLOC` and `FREED` to `TOP`; retain that MAY semantics rather than asserting definite freed state. However, caller-snapshot restoration cannot be used to erase an observed callee effect or manufacture unrelated identity. The gate must check before/after domain snapshots, event identity, and actual exported allocation state on `R`.

The unscoped legacy local representation needs an opt-in coordinated scope adapter or transfer view keyed through the accepted P2 activation. Preserve the same `CellValue` lattice and frozen non-P3 behavior. A plan that can only repair identity in `cqpl_export` fails this protocol.

## 10. Callee-created allocations

Only existing modeled allocation operations may create resources. `memory_events::rust_allocation_semantics` distinguishes `Fresh`, `MayFreshOrTransfer`, `Transfer`, `ConditionalReallocation`, and `None`; these frozen classifications remain authoritative. A transfer call is not a fresh allocation. A conditional realloc is not MUST-fresh.

A modeled dependency allocation uses its current context-qualified `AllocationSiteId` and existing `AbstractAllocId.context`. Retain a compiler-origin proof to the exact Instance/code site. The returned value receives the same ID. Version, trait implementation, and concrete generic separation come from native Instance/P2 provenance, not display paths.

For MAY-fresh-or-transfer and conditional outcomes, retain the accepted alternatives and existing branch guards. Do not turn them into exact allocation or input-alias equality to simplify a fixture.

## 11. Repeated calls, aliases, and nested flow

For two distinct resources `R_A`, `R_B` entering the same canonical callee at `K_A`, `K_B`, validate separate activation-scoped formals, effects, returns, and caller destinations. The resource of activation A must not capture B merely because body acquisition is shared. Inspect actual domain annotations at both activations and caller continuations.

Conversely, if supported caller values `p` and `q` already denote the same `R`, passing both to formals must preserve that same abstract identity. Creating one resource per formal is not an acceptable non-conflation strategy.

For `app --K1--> dep --K2--> dep2`, contexts are `[]`, `[K1]`, `[K1,K2]`. The resource may retain its caller origin while crossing both boundaries. Each normal pop binds through the exact intermediate continuation; a second app-to-dep activation must not capture that flow. Generic Instances, concrete trait implementations, and duplicate versions remain orthogonal to the execution provenance and original resource identity.

## 12. Normal, unwind, divergence, recursion

**Normal:** only the exact normal P2 relation may bind `_0` to its matched destination. Side effects propagate according to supported ordered domain transfer.

**Unwind:** no normal return destination/resource is created. Supported operations already executed before unwind may affect the same caller resource at the exact cleanup under popped context. The current panic profile suppresses unrepresented call destinations, widens partial effects to `TOP`, and retains MAY partial/stale-owner facts. Preserve those policies; do not assert a precise dependency unwind resource theorem from the P2 graph alone. Unsupported required unwind flow emits `unsupported_unwind_resource_flow` and incompleteness. A normal/cleanup edge label alone is insufficient; consume the accepted P2 unwind object.

**Divergence:** entry/formal transfer may be meaningful, and modeled effects before divergence may be visible inside the activation. No normal `_0`, caller destination, or fake post-call resource binding exists. CQPL completion does not become a Rust normal return.

**Recursion:** reachable direct/mutual represented-Rust recursion remains `recursive_call_context_unsupported` with P2 incomplete. P3 must not truncate context, solve recursive resource summaries, or convert that graph into a complete model.

## 13. Analysis placement

Source currently runs:

```text
MIR -> P2 context_expand -> AbstractMemory/taint fixed point
    -> identity fixed point + disposition identity fixed point
    -> exporter invokes enabled panic lifecycle -> annotation
```

This is the measured ordering, including lifecycle calculation inside the current export routine. An older target-contract wording that lists identity first does not override source facts.

The proposed opt-in P3 pipeline is coordinated:

```text
MIR structural value evidence + accepted P2 G
    -> exact activation/value binding plan (no invented resource values)
    -> identity fixed point with certified entry/return relations
    -> scoped AbstractMemory/effect transfer using that plan and identity evidence
    -> lifecycle on the same G and corrected identity
    -> authoritative producer binding/state records
    -> read-only CQPL annotation/export
```

Identity is the appropriate place for allocation candidate preservation; abstract-domain edge transfer is the place for `CellValue` effects. P3 therefore spans those two existing producers. Both preserve their accepted lattices/profiles. CFG loops still require ordinary fixed points even when the represented call graph is acyclic. If dependencies between identity and state need coordinated iteration, its monotonicity/termination must be established in implementation rather than assumed here.

The exact implementation mechanism is reviewed at R1/R3. No export-time identity repair is allowed. Capability-disabled analysis ordering and output remain frozen.

## 14. Minimal proposed data model

The proposal extends producer evidence associated with `AllocationIdentityState`, rather than introducing another allocation catalog or a CQPL resolver. The actual type/API extension requires separate review before implementation.

A binding record uses:

- existing `call_key`, caller/callee native Instance IDs, ordered caller/callee P2 contexts, and exact activation reference;
- `relation_kind`, optional zero-based formal index and compiler-certified caller/callee `ProgramVarId`/`PlaceId`;
- ordered unique references to existing allocation catalog IDs, not a guessed scalar `resource_id`;
- exact input/normal/unwind P2 edge reference and fixed-point/event-boundary evidence;
- explicit status and certainty.

Origin provenance maps newly created existing Rust allocation IDs to native canonical code and their current execution site. It does not rekey passed resources or change static-site/context semantics. Session-only compiler objects are never serialized as timeless identity.

The new payload/capability is omitted when disabled. CQPL may validate referenced nodes/values/resources and capability closure. Its truth inputs remain authoritative existing `identity`, `event_identity`, `allocation_labels`, `allocation_post`, disposition, contracts, and lifecycle records. Sidecar self-consistency alone is insufficient acceptance.

## 15. CQPL and existing C/FFI preservation

CQPL allocation quantifiers bind declared opaque allocation IDs. Positive allocation MAY state/event predicates evaluate to `unk`; exclusion evaluates to `ff`. P3 does not turn equal abstract singleton candidates into runtime MUST truth or change CTL/assessment semantics.

Preserve the exact existing fields and producers for positional FFI identity, external formal effects, ERR1 returns, ND1/EFX1 deallocation, DCP1 call provenance, allocation existence guards, reallocation boundaries, conditional realloc success/failure, and ELE1 envelopes. Existing ERR1 return contracts are exported with ELE1 bindings; the historical separate return-binding wire field is suppressed in the current producer. Do not revive it as a second resolver.

The current declared capability strings are captured in `current-declared-resource-capabilities.json`. Identity itself is a schema-v2 surface; there is no current declared `allocation_identity_v1` capability. Preserve allocation contract/disposition versions and positive/negative evidence gates as they actually exist.

Replay established C/SVF controls from the known-good `crema/` working directory. The known `./src/svf-example` portability debt is not P3 semantics. Ambiguous multi-context foreign boundaries already rejected by P2 remain rejected; P3 must not clone/rewrite C bodies to bypass that limit.

Conditional-reallocation checker refinement and terminal quiescence keep their accepted state/event behavior and ordering. No new checker-local resource binding or Rust call resolver is permitted.

## 16. Status and completeness

These are proposed P3 statuses, not implemented producer claims.

| Status | Level | Meaning |
|---|---|---|
| `resource_binding_available` | Relation | Supported abstract transfer certified by exact producer evidence. |
| `resource_binding_unavailable` | Relation | Required supported transfer lacks evidence. |
| `unsupported_value_shape` | Value | Operand/place outside the implemented abstraction. |
| `unresolved_actual` | Value | Caller place cannot be certified. |
| `unresolved_formal` | Value | Callee layout/position cannot be certified. |
| `unresolved_return` | Normal relation | Expected resource return cannot be established. |
| `unresolved_resource_identity` | Relation | Candidate identity/origin is not established. |
| `unsupported_unwind_resource_flow` | Unwind relation | Required prior effects cannot be propagated soundly. |
| `no_tracked_resource` | Value | Positive scalar/nonresource classification; not empty-map fallback. |
| `p2_control_flow_incomplete` | Run | Accepted P2 prerequisite unavailable, including recursion. |
| `compiler_error` / `compiler_session_failed` | Run terminal | Compiler/session failure; no complete artifact or per-body fake recovery. |
| `resource_limit_reached` | Run | Fixed point/coverage truncated; incomplete. |

Keep `call_semantic_coverage_complete`, `p2_control_flow_complete`, and proposed `resource_continuity_complete_for_reachable_supported_bindings` separate. The resource field is true only when P2 is complete, fixed points finish, every required resource-relevant relation is certified, semantic ownership is valid, and no compiler/resource-limit failure occurred. A reachable unsupported required shape is incomplete; the word “supported” is not permission to omit it silently. Proven scalar/nonresource values are explicitly not applicable. Accepted alternative FFI/ELE1 ownership is evaluated under its unchanged contract.

No status here claims whole-program memory safety, full heap coverage, or recursive exactness. Fatal query failures remain terminal compiler-session failures under SC1; no recoverable per-body `compiler_query_failed` is invented.

## 17. Soundness invariants

- **I1 Callsite exactness:** every relation belongs to one accepted `(call_key, caller context)` activation.
- **I2 Resource preservation:** supported alias/pass-through keeps canonical abstract IDs.
- **I3 No cross-call capture:** shared canonical body cannot rebind A's resource to B.
- **I4 No false splitting:** supported aliases of one resource stay shared across formals.
- **I5 Return exactness:** destination is the same activation's matched normal destination.
- **I6 New origin:** callee-created resource retains its exact existing compiler-qualified site/context.
- **I7 Effect continuity:** effects target caller `R`, not fabricated formal resource.
- **I8 Normal/unwind separation:** no normal destination binding on unwind.
- **I9 Divergence:** no normal return-resource binding for a diverging activation.
- **I10 Compiler identity:** generic/trait/version distinctions do not use pretty-name equality.
- **I11 Ownership:** Rust continuity does not steal FFI/ELE1/uncovered calls.
- **I12 No CQPL compensation:** checker never repairs missing producer resource semantics.
- **I13 Disabled neutrality:** capability off preserves the immediate parent behavior.
- **I14 Determinism:** native IDs, resource values, bindings and states match A1/A2/B.
- **I15 Recursion fail-closed:** P3 cannot make an incomplete recursive P2 graph complete.

## 18. Fixture and adversarial acceptance

The following matrix is a design, not executed P0 evidence. Patterns must first survive actual pinned optimized-MIR extraction. `K` symbols stand for recomputed compiler/P2 call keys, not name-derived fixture IDs. Every row must retain raw MIR/value evidence, exact bodies and activations, input/output and event identity, actual domain annotations, exported allocation/state data, and real CQPL consequences where applicable. The detailed JSON specifies expected positive and negative bindings for each row.

| Control | Pattern / purpose | Required identity/binding consequence | Forbidden consequence |
|---|---|---|---|
| R1 | app: p=Box::into_raw(Box::new(1_u32)); dep::observe(p); dep: fn observe(p:*mut u32) { let _=p; } | actual_0@[] -> formal_1@[K1], points_to candidates preserved | fresh resource at formal; base index guessed for projection |
| R2 | A=Box::into_raw(Box::new(1)); B=Box::into_raw(Box::new(2)); dep::pass(A); dep::pass(B); #[inline(never)] fn pass(p:*mut u32)->*mut u32{p} | actual A -> formal@[K_A] -> _0@[K_A] -> destination A; analog B | formal/return/effect@[K_A] -> R_B; reverse |
| R3 | p=Box::into_raw(Box::new(1)); q=p; dep::observe_two(p,q); fn observe_two(p:*mut u32,q:*mut u32){ let _=(p,q); } | actuals 0,1 -> formals 1,2 same candidate set R | new resource per formal; equivalence closure through unrelated MAY overlap |
| R4 | r=dep::pass(p); use tracked r then original p; fn pass(p:*mut u32)->*mut u32{p} | _1 -> _0 -> exact caller destination with same R | fresh return resource; wrong callsite destination |
| R5 | dep: #[inline(never)] fn make()->*mut u32{Box::into_raw(Box::new(3))}; app:r=dep::make() | callee allocation -> _0 -> caller destination same whole AbstractAllocId | alias input/caller alloc; sibling/version/Instance alloc origin |
| R6 | app:p=Box::into_raw(Box::new(1)); dep::release(p); later supported caller use(p); dep: unsafe fn release(p:*mut u32){ drop(Box::from_raw(p)); } | actual/formal R; existing deallocation operation -> R; caller sees effect on R | new freed R_formal; missing effect; effect redirected to B |
| R7 | app:r=dep::pass(p); dep: fn pass(p:*mut u32)->*mut u32{dep2::pass(p)}; dep2: fn pass(p:*mut u32)->*mut u32{p} | Both actual/formal and both normal pops preserve R; add supported nested effect after alias version | lost outer context; dep2 effect/return into alternate dep activation |
| R8 | dep: fn pass<T>(p:*mut T)->*mut T{p}; app calls pass::<u32>(p), pass::<u64>(q) | Each actual/return attached to its concrete Instance/context | generic body substitution; resource duplicated on entry |
| R9 | trait Pass { fn pass(&self,p:*mut u32)->*mut u32; }; dep concrete Impl returns p; app concrete dispatch | Bindings attach Instance::try_resolve implementation, not declaration | trait declaration/wrong impl substitution |
| R10 | workspace renamed dependencies to duplicate package versions; each version has make() and pass(p) | Exact PackageId provenance -> native Instance -> allocation site; exact separate bindings | version/name-based merge; PackageId swap; resource origin substitution |
| R11 | two calls to same dep function with separate cleanup guards; dep performs supported effect then may panic; obtain actual optimized-MIR cleanup(bbN) | normal _0 -> normal destination only; prior effects on unwind -> exact cleanup with context pop | normal destination on unwind; wrong callsite cleanup/effect |
| R12 | dep: fn never(p:*mut u32)->! { loop { core::hint::black_box(p); } }; app calls represented diverging body | actual/formal if shape supported; pre-divergence modeled effects only | _0/destination binding; dummy normal return |
| R13 | eligible Rust expected MIR absent; separate foreign dep::abs/bodyless/uncovered boundary control | Existing FFI/ELE1 relations only when exact producer proves owner; otherwise explicit unavailable/uncovered | foreign/name -> ELE1 delegation; unavailable -> resource_binding_available |
| R14 | dep: #[inline(never)] fn recurse(p:*mut u32){recurse(p)}; optionally mutually recursive functions | p2_control_flow_incomplete with recursive_call_context_unsupported | bounded context; recursive graph P3 complete |

R2 requires independently testing effect and return capture, not just counting records. R3 requires the converse shared-resource test. R6 uses actual supported deallocation evidence: dropping a raw pointer is not freeing its pointee. R11 requires real pinned-MIR cleanup, supported pre-unwind effect propagation, and a negative incomplete control for unsupported shapes. R12 preserves entry/formal identity without normal return. R13 requires both missing eligible Rust-body and exact foreign-ownership controls, without fitting `dep::abs`. R14 is a fail-closed test, never a bounded-recursion success.

The existing MAY oracle remains unchanged: positive allocation use/drop/state predicates can be `unk`; identity equality of exported abstract IDs is a different proposition from concrete memory-safety truth. Record actual CQPL result and assessment; do not force `tt` by changing semantics.

### Independent adversarial plan

Plan 36 valid-JSON semantic mutations. For each, preserve original/mutated input, independently confirm the intended typed field was changed, and reject by recomputation from raw compiler operands, P2 provenance, identity/event/state snapshots and final annotations. A parse error or self-inconsistent ledger alone does not establish resource soundness. No mutations were executed in P0.

- **A01**: actual A rebound to resource B.
- **A02**: formal K_A rebound to resource from K_B.
- **A03**: return K_A sent to caller destination K_B.
- **A04**: same-resource aliases falsely split.
- **A05**: distinct resources falsely merged.
- **A06**: callee allocation assigned caller origin.
- **A07**: caller allocation assigned callee origin.
- **A08**: nested dep2 relation loses outer context.
- **A09**: generic Instance substituted.
- **A10**: trait declaration substituted for implementation.
- **A11**: duplicate version substituted.
- **A12**: callee deallocation applied to fabricated fresh resource.
- **A13**: deallocation of R disappears from caller R state.
- **A14**: normal return resource created on unwind.
- **A15**: unwind effect redirected to wrong activation.
- **A16**: diverging call gets return binding.
- **A17**: body_unavailable gets fabricated binding.
- **A18**: CREMA FFI owner stolen by Rust path.
- **A19**: ELE1 owner stolen by Rust path.
- **A20**: actual/formal indices swapped.
- **A21**: returned alias converted to fresh allocation.
- **A22**: new allocation converted to input alias.
- **A23**: compiler origin replaced by pretty name.
- **A24**: execution context silently redefines canonical allocation abstraction.
- **A25**: recursive graph incorrectly P3 complete.
- **A26**: CQPL-only fabricated resource relation.
- **A27**: empty unresolved pointer candidates claimed no_tracked_resource.
- **A28**: derived access-only alias promoted to base-free certificate.
- **A29**: unsupported projection rebound to base-local resource.
- **A30**: event identity lost by later same-BB overwrite.
- **A31**: LegacyFrozen/Disposition profile switched silently.
- **A32**: multi-resource MAY set replaced with arbitrary singleton.
- **A33**: root compiler/session failure marked complete.
- **A34**: disabled mode emits P3 field or changed state.
- **A35**: same site/context loop creates artificial dynamic object identities.
- **A36**: G call/return relation changed only in resource sidecar.

Implementation closure requires all planned mutations rejected, zero unexpected accepts, and zero unrelated parse errors. A verifier may add mutations but may not weaken these. Mutation detection must include actually consumed resource annotations/state, not solely relation sidecars.

## 19. Staged gates and exact outcomes

Each gate requires independent review and separate implementation authorization. P0 does not advance itself.

| Gate | Authorized scope | Acceptance | Incomplete |
|---|---|---|---|
| P3-P0-RIC0, current | Source audit and protocol only | `P3_P0_PROTOCOL_READY_FOR_REVIEW` | `P3_P0_RESOURCE_MODEL_BLOCKER` or `P3_P0_PRECONDITION_FAILED` |
| P3-R1 | Structural actual/formal proof, safe scoped boundary transfer, R1/R2 entry/R3/R12 entry/R13 | Proposed `P3_R1_ACTUAL_FORMAL_ACCEPTED` | `P3_R1_INCOMPLETE` |
| P3-R2 | Exact normal return, alias vs fresh callee origin, R2/R4/R5/R12 negative | Proposed `P3_R2_RETURN_CONTINUITY_ACCEPTED` | `P3_R2_INCOMPLETE` |
| P3-R3 | Caller-visible effects and normal/unwind propagation, R6/R11 | Proposed `P3_R3_EFFECT_CONTINUITY_ACCEPTED` | `P3_R3_INCOMPLETE` |
| P3-R4 | All 14 controls, nested/generic/trait/version/recursion, preservation, adversarial/determinism/disabled closure | `P3_FINAL_RESOURCE_IDENTITY_ACCEPTED` only after all earlier gates accepted | `P3_R4_INCOMPLETE` |

R1 must resolve the scope/certificate requirements needed by its subset; it cannot declare a skipped pointer shape resolved. R2 cannot infer return identity from an allocation name or create resources on no-return paths. R3 must make supported effects visible in producer states while preserving the lattice; any unsupported required unwind flow remains incomplete. R4 must rerun all earlier controls on its final source.

If a resource-model limitation requires changing the accepted abstraction, stop and produce its exact compiler/domain counterexample for separate review. Do not silently reinterpret resource identity to pass the gate.

## 20. Determinism and disabled-mode validation plan

After final production edits, run A1 and A2 with fresh target directories in one source root and B with equivalent sources and an independently built executable at a different absolute path. Use accepted production `RUSTC_WRAPPER` invocation and unchanged wrapper conflict policy.

Compare native StableCrateId/DefPathHash/Instance values, call keys, ordered P2 contexts, whole `AbstractAllocId` values, certified origins, positional/place actual/formal bindings, exact return/effect relations, state transitions, and final annotated resource/event identities. No native identity normalization. Exclude only explicitly run-local diagnostic paths. Existing site/context representation is part of semantic equality, not a disposable path diagnostic.

The pre-P3 disabled reference must be captured from the exact clean immediate parent named above, before production edits in the separately authorized implementation phase. Preserve command, environment, feature/configuration inputs, G, annotated artifact, queries/results/assessments, and hashes. With P3 disabled, compare deterministic graph/export bytes where appropriate, otherwise a predeclared exact semantic/serialization projection. No P3 capability or binding sidecar may appear and no accepted existing resource state may change. Historical DEP1-disabled comparisons remain separately labeled. The final 83 x 12 matrix is reserved for its later authorized gate.

## 21. Evidence and regression obligations

For each implementation gate preserve full source provenance/patch, exact compiler/Cargo invocation, metadata/artifacts, MIR actual/formal/return evidence, native identity, P2 G and activation table, pre/post/event identity states, abstract/taint/lifecycle states, final A, existing coverage owners, actual CQPL query/result/assessment, verifier inputs/results, and declared supported/unresolved shapes.

Run pinned cargo check and focused affected tests. Run full CREMA after producer changes and full CQPL when its serialized API/export changes. Include real production-artifact CQPL queries and preservation controls for C/FFI/ELE1, reallocation, quiescence, no-return, and recursion. Report measured counts and failures; historical counts are not acceptance oracles.

P0 requires the reports listed in its evidence root: precondition, source audit, abstract semantics, identity layering, relation specification, analysis order, CQPL consumption, preservation map, status contract, fixture matrix, adversarial plan, gate plan, source hashes, initial/final Git status and diff check, and exact source excerpts. A proposed data-model report and validation report are included. P0 does not run a production implementation suite because no production source/schema changes are authorized or made.

Generate package-relative SHA256SUMS only after all artifacts and logs close. Verify from inside the evidence root. No live console writer belongs in the manifest. Accepted older evidence is referenced with hashes, not rewritten.

## 22. Held-out evaluation and stop condition

RUSTSEC-2026-0128 / emap remains held out throughout design and synthetic implementation validation. It supplies neither fixture expectations nor classifiers. Only after separately accepted and frozen `P3_FINAL_RESOURCE_IDENTITY_ACCEPTED` may a separately authorized external evaluation run occur.

P0 READY means this protocol and source audit are ready for review; it does not mean that P3 resource continuity exists, that memory safety is verified, or that full DEP1 is complete. Stop after P0 for independent review. No commit or push.

## Appendix A. Source-exact anchors

The following declarations anchor the claims. Complete numbered source and SHA256 are in `RESOURCE_MODEL_SOURCE_AUDIT.json` and `source-excerpts/`. Declaration lines are not a substitute for the preserved function bodies.

| Source | Declaration line | Semantic role |
|---|---:|---|
| `crema/src/structs.rs` | 19 (`ProgramVarId`) | Program variable identity |
| `crema/src/structs.rs` | 110 (`AbstractAllocId`) | Resource/allocation identity |
| `crema/src/structs.rs` | 148 (`AllocationSiteId`) | Origin |
| `crema/src/structs.rs` | 166 (`PlaceId`) | Projected value/reference |
| `crema/src/structs.rs` | 195 (`RustCallMetadata`) | Positional actual and normal return relation |
| `crema/src/structs.rs` | 221 (`MirCallArgument`) | Existing actual record |
| `crema/src/identity.rs` | 137 (`AllocationIdentityMemory`) | Authoritative MAY identity domain |
| `crema/src/identity.rs` | 2050 (`bind_actuals_to_formals`) | Local actual to formal _i+1 transfer |
| `crema/src/identity.rs` | 2078 (`bind_return_to_caller`) | _0 to matched destination and caller snapshot join |
| `crema/src/identity.rs` | 605 (`bounded_context`) | Allocation analysis context |
| `crema/src/identity.rs` | 2222 (`fixed_point_identity_analysis_with_profile`) | Matched identity worklist |
| `crema/src/abstract_domain.rs` | 81 (`CellValue`) | Memory state lattice |
| `crema/src/abstract_domain.rs` | 176 (`AbstractMemory`) | Legacy memory transfer domain |
| `crema/src/abstract_domain.rs` | 3895 (`bind_internal_actuals`) | Legacy actual/formal transfer |
| `crema/src/abstract_domain.rs` | 3917 (`fixed_point_analysis`) | Memory/taint worklist on G |
| `crema/src/abstract_domain.rs` | 1942 (`apply_mir_terminator_for_edge_enabled`) | Unwind/normal state transfer |
| `crema/src/execution_graph.rs` | 125 (`context_expand`) | Accepted P2 authoritative graph |
| `crema/src/panic_lifecycle_domain.rs` | 890 (`fixed_point_real_panic_lifecycle`) | Edge-based owner state propagation |
| `crema/src/panic_unwind.rs` | 37 (`edge_flow_kind`) | Existing edge classification adapter |
| `crema/src/cqpl_export.rs` | 2225 (`stable_allocation_id`) | Persistent resource serialization |
| `crema/src/cqpl_export.rs` | 3501 (`allocation_memory_annotation`) | Current lifecycle state lift |
| `crema/src/cqpl_export.rs` | 4119 (`external_library_effect_envelopes`) | ELE1 bodyless envelope |
| `cqpl/cqpl_checker/src/kripke.rs` | 1985 (`allocation_may_hold`) | Resource-state truth |
| `cqpl/cqpl_checker/src/kripke.rs` | 2017 (`allocation_label_hold`) | Resource event truth |
| `crema/src/cqpl_export.rs` | 4228 (`dep1_call_semantic_coverage`) | COV1 exact call owner |
| `crema/src/cqpl_export.rs` | 4119 (`external_library_effect_envelopes`) | ELE1 exact bodyless binding and nonempty envelope |

Additional pipeline anchors: `crema/src/main.rs:420–485` and `728–792`; structural MIR operand extraction: `crema/src/icfg.rs:982–1000`; accepted Instance/call-key helpers: `crema/src/icfg.rs:1344–1363`; allocation/disposition capabilities: `crema/src/cqpl_export.rs:1078–1155`.
