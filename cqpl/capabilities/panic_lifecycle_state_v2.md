# `panic_lifecycle_state_v2`

`panic_lifecycle_state_v2` refines `panic_lifecycle_state_v1` with an explicit
node-local producer-coverage frontier. An artifact declaring v2 must also
declare v1 and must carry both fields on every schema-v2 node:

- `panic_lifecycle`: sparse positive MAY records from v1;
- `panic_lifecycle_coverage`: one of `complete` or `unresolved`.

The coverage component is independent of the lifecycle MAY facts:

- `complete` means every lifecycle-relevant operation represented on the
  abstract path to this node was interpreted by the producer;
- `unresolved` means at least one relevant operation lost the correlation
  needed to update the lifecycle domain soundly.

For an allocation `a`, CQPL `repeat_drop(a)` has exactly this v2 semantics:

| positive MAY witness | coverage | result |
| --- | --- | --- |
| yes | either | `unk` |
| no | `complete` | `ff` |
| no | `unresolved` | `unk` |

`tt` is unavailable in v2 because the producer exports no MUST witness.

This preserves the framework's ordinary MAY interpretation: positive abstract
membership is unknown; exclusion is false only when the producer states that
its coverage is complete. Producer incompleteness is represented explicitly,
not inferred from absence of facts.
