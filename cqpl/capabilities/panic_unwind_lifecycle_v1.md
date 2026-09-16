# `panic_unwind_lifecycle_v1`

`panic_unwind_lifecycle_v1` is the A3 opt-in CREMA capability for
**edge-sensitive panic/unwind lifecycle propagation**.

It requires:

```text
schema_version = 2
mir_semantics_v2
mir_semantic_labels_v1
```

and is emitted only when CREMA is run with:

```bash
--cqpl-schema-version 2 --mir-semantics-v2 --panic-unwind-lifecycle-v1
```

## Problem addressed

The legacy fixed point applies a MIR terminator once at the source basic block
and then propagates that single post-state to every successor.  For fallible
terminators this collapses normal return and unwind cleanup into the same
abstract state.

The A3 profile instead evaluates the terminator on each outgoing ICFG edge:

```text
state_after_statements(bb)
        |\
        | \__ normal edge -> transfer_normal(term)
        |
        \____ unwind edge -> transfer_unwind(term)
```

## v1 transfer contract

### `Call`

Normal continuation
: For a represented Rust/FFI callee, entering the callee is an identity edge;
  the callee body owns its effects and the existing matched return binding maps
  the return value only after a successful return. For a summary call, the
  existing CREMA call transfer is reused.

Unwind continuation
: A represented Rust callee propagates its exceptional state from explicit
  `UnwindResume` or `unwind=Continue` exits into the caller cleanup. A summary
  call does not assign the MIR destination/return place; tracked arguments are
  conservatively widened to `TOP` because the omitted callee may have partially
  mutated or dropped them before panicking.

### `Drop`

Normal continuation
: Reuses the existing `Drop -> FREED` transfer.

Unwind continuation
: The dropped allocation is `TOP`, not definitely `FREED`.  This represents a
partially executed destructor until a richer field-/element-sensitive lifecycle
domain is available.

### `Assert`

The unwind continuation preserves the incoming memory state: the failed assert
causes the panic before the success continuation.

### `InlineAsm`

An unwind continuation widens all tracked memory to `TOP`.

## Soundness boundary

This capability removes the legacy normal/unwind state collapse, but v1 is
intentionally conservative.  In particular it does **not** claim to reconstruct
partially completed element-drop loops or commit-before-drop guards beyond what
the current MIR statements and `CellValue` domain can represent. Reachable
dependency MIR is imported only when rustc reports it as available; otherwise
the call remains a conservative external summary.

Therefore:

- `TOP`/`unk` is an expected safe result when the unwind-side effect is not
  representable in the current `CellValue` domain;
- absence of a vulnerability witness on an external summary is not evidence of
  safety;
- benchmark cases that depend on dependency-internal unwind logic must verify
  that the vulnerable and fixed dependency bodies are actually present in the
  analyzed ICFG before treating a CQPL differential as causal evidence;
- interprocedural unwind is currently context-insensitive at exceptional exits,
  so multiple callsites of the same represented callee can join; this is a MAY
  overapproximation and a documented precision boundary, not a safety proof.

## Compatibility

The serialized ICFG currently retains the frozen human-readable edge labels
(`Call unwind`, `Drop unwind`, `Assert unwind`, `InlineAsm unwind`).  CREMA maps
those labels to a typed internal `EdgeFlowKind` adapter.  This avoids changing
the frozen ICFG JSON schema while making the dataflow transfer edge-sensitive.
