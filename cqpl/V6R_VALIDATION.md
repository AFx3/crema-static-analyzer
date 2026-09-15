# v6R-r1 — validation e freeze evidence

v6R-r1 è accettato solo se l'explainability è **osservazionale**: i truth result congelati v6Q-r1c non devono cambiare.

Baseline commit:

```text
bbca5f09096d77718624564e0aafff9d87a96e6e
```

Toolchain validato:

```text
nightly-2024-11-21
rustc 1.84.0-nightly (3fee0f12e 2024-11-20)
```

## Stato runtime validato

Run del 2026-09-15:

```text
subjects=112
queries=12
attempts=1344
baseline_result_mismatches=0
failures=0
ff=650
unk=468
tt=226

unknown_without_reason_frontier=0
unknown_without_specific_origin=0
true_without_witness=0
true_without_atomic_witness=0

leak_alloc unknown=105/112; MAY_ALLOCATION=105/105
leak_alloc_state unknown=105/112; MAY_ALLOCATION=105/105

reviewed_positive_nonrefutation:
ML=33/33
DF=29/29
UAF=22/22
UB_FFI=18/18
```

Runtime evidence archive esterno:

```text
explainability.zip
sha256=f41117d1d2fc1838cc1ee830071b07673811765884c9d96e4229ebbd187247d4
explanation_json_files=1344
```

Il bundle runtime non è sorgente da committare; il repository conserva il summary in `artifact/V6R_RUNTIME_VALIDATION.json`.

## Gate A — boundary statico osservazionale

`verify_v6r_static.py` verifica:

- 9 file CREMA byte-identici alla baseline;
- 12 query `queries_v2` byte-identiche;
- AST, parser, Kripke e truth semantics byte-identici;
- `model_checker.rs` semanticamente identico salvo le due visibility promotion usate dall'explainer;
- delta CQPL esatto uguale a `artifact/V6R_STAGE_PATHS.txt`;
- manifest completo;
- conteggi baseline leak/precision coerenti;
- record runtime v6R coerente con 1344/1344 e zero mismatch.

## Gate B — compile e test

```bash
cargo +nightly-2024-11-21 test \
  --manifest-path cqpl/cqpl_checker/Cargo.toml
```

Run validato:

```text
57 lib tests passed
7 main tests passed
4 v6i tests passed
0 failed
```

I due warning (`StructuralLabelKind` unused e `Trace::merge` dead code) non cambiano il risultato e sono stati lasciati invariati per non alterare i byte del source runtime-validato durante il freeze documentale.

## Gate C — 112 x 12 result identity

Acceptance:

```text
attempts_completed = 1344
baseline_result_mismatches = []
failures = []
unknown_without_reason_frontier = []
unknown_without_specific_origin = []
true_without_witness = []
true_without_atomic_witness = []
```

Run validato: PASS.

## Gate D — leak frontier

Entrambe le query leak devono preservare:

```text
subjects = 112
unknown = 105
MAY_ALLOCATION frontier = 105 / 105
```

Run validato: PASS.

`reason_presence` contiene conteggi sovrapposti. `signature_counts` è la partizione per signature completa della frontier.

## Gate E — reviewed oracle

```text
ML     reviewed positive 33, unexpected_ff=0
DF     reviewed positive 29, unexpected_ff=0
UAF    reviewed positive 22, unexpected_ff=0
UB_FFI reviewed positive 18, unexpected_ff=0
```

Run validato: PASS.

## Ripetere la validation completa

Per un candidate estratto prima dell'installazione:

```bash
set +e
ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21
CAND=/path/to/crema_cqpl_v6r_r1_final_freeze_candidate
BASE="$ROOT/repro-results/cqpl-v6q-r1c-final112"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="$ROOT/repro-results/cqpl-v6r-r1-validation-$STAMP"
LOG="$OUT.console.log"

CAND="$CAND" \
LOG="$LOG" \
CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
V6R_BASE="$BASE" \
V6R_OUT="$OUT" \
bash -o pipefail -c '"$CAND/validate_v6r_candidate.sh" 2>&1 | tee "$LOG"'

RC=$?
echo "v6r_validation_rc=$RC"
```

Marker finali:

```text
V6R_STATIC_VERIFY: PASS
V6R_SUBJECT_REBASE: PASS subjects=112
V6R_EXPLAINABILITY_MATRIX: PASS
V6R_LEAK_UNKNOWN_AUDIT: PASS
V6R_R1_VALIDATION: PASS
v6r_validation_rc=0
```

## Freeze documentale successivo alla validation runtime

Il final freeze package aggiorna documentazione e metadata, ma **non cambia i file Rust runtime-validati dell'explainability**. Per questo il freeze richiede una prova hash fra source del candidate runtime-validato e source del final freeze package; non è necessario ripetere 1344 query se tale boundary passa.

La validation completa può comunque essere rieseguita volontariamente come replica indipendente.


## Static verifier execution modes (freeze protocol r1a)

The static verifier has two deliberately different filesystem scopes.

- **standalone-package**: selected when the parent of `cqpl/` contains `CANDIDATE_SHA256SUMS`. Package hygiene is checked recursively because every file below that directory belongs to the distributable candidate.
- **installed-tree**: selected when `cqpl/` lives inside the working repository. The verifier checks the CQPL delta, manifest, semantic/runtime hash boundaries and documentation invariants, but it does not recursively classify unrelated repository-local `target/` or `repro-results/` trees as shipped package files. Commit hygiene is instead enforced on the exact Git staged file set.

This distinction is a validation-protocol correction only. It does not modify the runtime-validated Rust implementation or any frozen query.
