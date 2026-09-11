# CREMA isolated regression harness v4

Phase-4.4 regression harness.

Scientific invariants:
- exact selector `nightly-2024-11-21`;
- per-target isolated cleanup/build;
- exactly 92 executed targets;
- `openapi-client-gen` excluded by protocol;
- normalized semantic comparison against Phase 3.1;
- only `shared-register` accepts ML or DF+ML;
- Phase-4.4-aware MIR census.

The census now positively recognizes:
- `closure_aggregate`;
- `copy_for_deref`;
- `direct_deref_use`.

Full run:

```bash
./crema_repro_baseline_clean_v4/run_crema_isolated_full.sh \
  repro-results/boxtimes-phase4_4-full
```

Semantic validation:

```bash
python3 crema_repro_baseline_clean_v4/compare_crema_semantics_92.py \
  repro-results/boxtimes-phase3_1-full/results.normalized.json \
  repro-results/boxtimes-phase4_4-full/results.normalized.json

python3 crema_repro_baseline_clean_v4/validate_phase4_4_full.py \
  repro-results/boxtimes-phase3_1-full/results.normalized.json \
  repro-results/boxtimes-phase4_4-full
```
