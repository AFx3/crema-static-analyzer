# CQPL schema-v2 demo — `boxed_bool`

Questa demo esegue le 12 query ufficiali CQPL v2 sul target Rust
`boxed_bool` e mostra il contratto di explainability v6T-r1.

Per ogni query:

- `tt` e `ff` conservano il normale output;
- `unk` produce obbligatoriamente un file `*.explain.json`;
- con `--explain-unk-verbose`, ogni `unk` stampa anche una spiegazione
  umana completa;
- il report diagnostico non modifica la truth semantics three-valued.

## 1. Esecuzione completa delle 12 query

```bash
set +e

ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21

TARGET_REL="a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool"
OUT=/tmp/cqpl-v6t-boxed-bool-demo

rm -rf "$OUT"

python3 \
  "$ROOT/cqpl/scripts/run_one_target_v6q_r1c.py" \
  --root "$ROOT" \
  --relative-path "$TARGET_REL" \
  --out "$OUT" \
  --toolchain "$NIGHTLY" \
  --explain-unk-verbose

RC_RUN=$?

echo
echo "cqpl_demo_run_rc=$RC_RUN"
echo "output=$OUT"
```

Per il target `boxed_bool`, il profilo validato produce 12 risultati.
Le query leak restano `unk` sotto la semantica MAY congelata, ma il
report può contenere forte evidenza astratta di leak sul normal-return
path.

Un report UNKNOWN è del tipo:

```text
================================================================================
QUERY: leak_alloc_state
================================================================================
truth: unk

why unknown:
  - MAY_ALLOCATION
  - QUERY_THREE_VALUED_PROPAGATION

supporting findings:

  kind       : normal_return_open_manual_obligation
  strength   : strong_abstract_evidence
  handoff    : rust::main::bb4
  return     : rust::main::bb5
  path:
    -> rust::main::bb4
    -> rust::main::bb5

  evidence:
    - producer_certified_box_into_raw
    - normal_return_reachable
    - no_modeled_discharge_on_witness_path
    - no_intervening_call_after_handoff
    - non_returning_discharge_observed_off_witness

uncertainty witnesses:
  witness 1: truth=unk
     rust::main::bb0 | alloc(a) = unk | MAY_ALLOCATION
```

`strong_abstract_evidence` non è una prova di esecuzione concreta e non
promuove `unk` a `tt`.

## 2. Verifica automatica della closure UNKNOWN

Il runner genera:

```text
query-results.tsv
unknown-explanations.tsv
unknown-explanations-summary.json
queries/<query>.explain.json
```

Controllo:

```bash
python3 - "$OUT" <<'PY'
import csv
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])

with (root / "query-results.tsv").open(newline="") as f:
    rows = list(csv.DictReader(f, delimiter="\t"))

unknown = [
    row for row in rows
    if row["result"] == "unk"
]

summary = json.loads(
    (root / "unknown-explanations-summary.json").read_text()
)

errors = []

if summary["unknown_results"] != len(unknown):
    errors.append("unknown result count mismatch")

if summary["explanations_generated"] != len(unknown):
    errors.append("explanation count mismatch")

if summary["complete"] is not True:
    errors.append("complete != true")

print("queries                =", len(rows))
print("unknown_results        =", len(unknown))
print(
    "explanations_generated =",
    summary["explanations_generated"]
)
print("complete               =", summary["complete"])
print("errors                 =", len(errors))

for error in errors:
    print("ERROR:", error)

if errors:
    print("CQPL_UNKNOWN_EXPLANATION_CLOSURE: FAIL")
else:
    print("CQPL_UNKNOWN_EXPLANATION_CLOSURE: PASS")
PY
```

Il contratto è:

```text
number of UNKNOWN results
    ==
number of validated explanation reports
```

## 3. Esecuzione diretta di una singola query

Il checker può essere usato direttamente.

```bash
set +e

ROOT=/home/af/Documenti/a-phd
ART="$OUT/annotated_icfg_v2.json"
QUERY="$ROOT/cqpl/queries_v2/leak_alloc_state.cqpl"

CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"

JSON_OUT=/tmp/cqpl-leak.stdout.json
VERBOSE_OUT=/tmp/cqpl-leak.stderr.txt

rm -f "$JSON_OUT" "$VERBOSE_OUT"

"$CHECKER" \
  "$ART" \
  "$QUERY" \
  --json \
  --explain-unk-verbose \
  >"$JSON_OUT" \
  2>"$VERBOSE_OUT"

RC_QUERY=$?

echo "query_rc=$RC_QUERY"

echo
echo "=== machine-readable stdout ==="
python3 -m json.tool "$JSON_OUT"

echo
echo "=== human UNKNOWN explanation ==="
cat "$VERBOSE_OUT"
```

Con `--json`, stdout resta JSON puro. Il report umano di
`--explain-unk-verbose` viene scritto su stderr, quindi gli script che
parsano stdout non vengono compromessi.

Per un risultato `ff` o `tt`, `--explain-unk-verbose` non stampa un
report UNKNOWN.

## 4. Interpretazione dei supporting findings

I principali finding diagnostici v6T-r1 sono:

```text
leak
    normal_return_open_manual_obligation

use-after-free
    drop_then_use_without_reallocation

double-free
    repeated_drop_without_reallocation

allocator mismatch, famiglie note
    allocator_family_mismatch

allocator contract non completamente risolto
    unresolved_allocator_contract_candidate
```

Le strength hanno significato distinto:

```text
strong_abstract_evidence
    il modello astratto contiene evidenza forte per il pattern

observational_candidate
    il pattern è osservato, ma provenance/contratti/astrazione
    non consentono una conclusione altrettanto forte
```

Un risultato `unk` può anche non avere alcun supporting finding positivo.
In quel caso la spiegazione della frontier resta comunque obbligatoria:
`unk` significa evidenza astratta insufficiente per un verdetto definito,
non un bug automaticamente confermato.

## 5. Semantica

Explainability e supporting findings sono diagnostica read-only.

Non cambiano:

```text
tt
ff
unk
```

e non riscrivono la semantica delle formule CQPL.
