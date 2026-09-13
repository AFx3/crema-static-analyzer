# allocation_state_v1

`allocation_state_v1` exposes a MAY abstract post-state indexed by
`AbstractAllocId` in schema-v2 annotated ICFG artifacts.

For each node `b` and allocation `a`, CREMA materializes:

```text
Pi#_post,alloc(b)(a)
  = join { Pi#_post(b)(v) | a in MayId#_post(b)(v) }.
```

The join is the existing `CellValue::join`; this capability does not introduce
a new lattice, a MUST analysis, or a new fixed point.

A query that applies `alloc`, `drop`, or `own_forg` to an allocation-bound
logical variable must declare:

```cqpl
requires allocation_state_v1;
```

Positive MAY membership evaluates to `unk`; exclusion evaluates to `ff`.

Artifacts declaring the capability must provide `allocation_post` on every
node. Missing or malformed state is a hard boundary error.
