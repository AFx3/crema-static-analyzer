# Guida operativa — analizzare target e crate con CREMA + CQPL R2

Questa guida descrive la procedura riproducibile usata dalla release final112.


Per il significato scientifico dei layer, leggere prima [ANALYSIS_PIPELINE.md](ANALYSIS_PIPELINE.md).

La baseline corrente da preservare è:

```text
112 subjects x 12 queries = 1344
ff=705  unk=413  tt=226
```

Per un `unk`, il JSON corrente aggiunge anche `assessment.subresult`, `direction`, `strength`, `basis` e `caveats`. Questi campi spiegano l'evidence disponibile ma non cambiano il `result`.

## 1. Prerequisiti

Assumendo:

```bash
ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21
```

verificare:

```bash
rustup run "$NIGHTLY" rustc --version
cargo +"$NIGHTLY" --version
test -x "$ROOT/SVF-example/src/svf-example"
echo "svf_rc=$?"
```

Atteso:

```text
rustc 1.84.0-nightly (3fee0f12e 2024-11-20)
svf_rc=0
```

Per i target che usano C/FFI, CREMA deve essere eseguito con working directory `$ROOT/crema`; alcuni path SVF sono risolti relativamente a quel contesto.

## 2. Via raccomandata: un target del corpus

```bash
python3 "$ROOT/cqpl/scripts/run_one_target_v6q_r1c.py" \
  --root "$ROOT" \
  --relative-path 'a-code_full_rust/a-double_free_full_rust_literals/boxed_bool' \
  --explain-unk-verbose
```

Output di default:

```text
$ROOT/repro-results/cqpl-v6q-r1c-one-<target>-<timestamp>/
```

File principali:

```text
build.log
crema-export.log
annotated_icfg_v2.json
allocation_identity.json
ffi_functions.json
query-results.tsv
query-results.json
unknown-explanations.tsv
unknown-explanations-summary.json
queries/*.explain.json
summary.json
SHA256SUMS
queries/*.json
queries/*.stderr.log
```

### Esempio FFI

```bash
python3 "$ROOT/cqpl/scripts/run_one_target_v6q_r1c.py" \
  --root "$ROOT" \
  --relative-path 'a-code_c_ffi/branch_df_mem_leak_ffi'
```

Lo script applica automaticamente gli eventuali override congelati presenti in:

```text
artifact/TARGET_ANALYSIS_CONFIG.tsv
regression/reference/entry_overrides.json
```

## 3. Procedura manuale equivalente

### 3.1 Scegliere il target

Esempio:

```bash
TARGET="$ROOT/tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals/boxed_bool"
OUT="$ROOT/repro-results/manual-boxed-bool-v6q-r1c"
mkdir -p "$OUT"
```

### 3.2 Compilare prima il target

```bash
cargo +"$NIGHTLY" build \
  --manifest-path "$TARGET/Cargo.toml" \
  --locked
```

La build preliminare è necessaria perché la fase FFI/extraction si aspetta il Cargo target directory del progetto.

### 3.3 Costruire il checker

```bash
cargo +"$NIGHTLY" build \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml"
```

### 3.4 Eseguire CREMA

```bash
bash -o pipefail -c '
  cd "'"$ROOT"'/crema" || exit 90

  cargo +"'"$NIGHTLY"'" run --manifest-path "'"$ROOT"'/crema/Cargo.toml" -- \
    "'"$TARGET"'" \
    --mir-semantics-v2 \
    --only-icfg-annotated \
    --cqpl-schema-version 2 \
    --annotated-icfg-out "'"$OUT"'/annotated_icfg_v2.json" \
    --allocation-identity-out "'"$OUT"'/allocation_identity.json" \
    2>&1 | tee "'"$OUT"'/crema-export.log"
'

RC_CREMA=$?
echo "crema_rc=$RC_CREMA"
```

Per target con configurazione esplicita aggiungere `--cargo-target ...`; per entry override aggiungere `--entry ...` esattamente come nel protocollo frozen.

### 3.5 Verificare il profilo

```bash
grep -F 'mir_semantics_profile=v2-extension-over-v6O' \
  "$OUT/crema-export.log"

test -s "$OUT/annotated_icfg_v2.json"
echo "artifact_rc=$?"
```

### 3.6 Eseguire una query

```bash
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"

"$CHECKER" \
  "$OUT/annotated_icfg_v2.json" \
  "$ROOT/cqpl/queries_v2/double_free_alloc_state.cqpl" \
  --json \
  --explain-unk-verbose
```

### 3.7 Eseguire tutte le query

Per una run interattiva usare il runner con il verbose UNKNOWN:

```bash
python3 "$ROOT/cqpl/scripts/run_one_target_v6q_r1c.py" \
  --root "$ROOT" \
  --relative-path 'a-code_full_rust/a-double_free_full_rust_literals/boxed_bool' \
  --toolchain "$NIGHTLY" \
  --explain-unk-verbose
```

Il protocollo finale tratta `rc != 0` come errore, non come truth value. Per ogni query v2 che ritorna `unk`, il runner genera comunque un sidecar `queries/<query>.explain.json` e aggiorna `unknown-explanations.tsv` / `unknown-explanations-summary.json`; questa closure è obbligatoria anche senza il flag verbose. Il flag controlla solo la stampa umana immediata del report.

## 4. Analisi di una crate library

Il pipeline library richiede una API root esplicita.

Esempio equivalente al soggetto `memchr 2.7.4`:

```bash
SOURCE=/path/to/memchr-2.7.4
OUT=/tmp/memchr-crema-analysis
mkdir -p "$OUT"

bash -o pipefail -c '
  cd "'"$ROOT"'/crema" || exit 90

  cargo +"'"$NIGHTLY"'" run --manifest-path "'"$ROOT"'/crema/Cargo.toml" -- \
    "'"$SOURCE"'" \
    --analysis-mode library \
    --cargo-kind lib \
    --api-root memchr::memchr::memchr \
    --target-triple x86_64-unknown-linux-gnu \
    --mir-semantics-v2 \
    --only-icfg-annotated \
    --cqpl-schema-version 2 \
    --annotated-icfg-out "'"$OUT"'/annotated_icfg_v2.json" \
    --allocation-identity-out "'"$OUT"'/allocation_identity.json" \
    --cargo-plan-out "'"$OUT"'/cargo_analysis_plan.json" \
    --semantic-coverage-out "'"$OUT"'/semantic_coverage.json" \
    --analysis-out-dir "'"$OUT"'" \
    --no-default-features \
    2>&1 | tee "'"$OUT"'/console.log"
'
```

Non sostituire automaticamente `lib` con `bin`: target, feature e root fanno parte del soggetto sperimentale.

## 5. Come interpretare i risultati

### `ff`

La formula è refutata nel modello astratto dalla entry selezionata.

Non significa automaticamente “programma sicuro”: refuta soltanto quella formula e quel modello.

### `unk`

La formula non è refutabile né stabilibile con l'informazione MAY.

Per query memory è normale e frequente. La run v6T produce automaticamente una spiegazione specifica per ogni `unk`. Interpretare separatamente:

- `reason_frontier`: perché la formula resta three-valued `unk`;
- `supporting_findings`: eventuale evidenza bug-specifica read-only;
- `strong_abstract_evidence`: pattern astratto completo per quella classe di bug, ma non prova MUST concreta;
- `observational_candidate`: pattern utile ma con origine/contratto ancora parzialmente irrisolti;
- nessun finding: `unk` indica solo evidenza astratta insufficiente, non un bug positivo.

Non promuovere mai `unk` a `tt` sulla base del solo diagnostic layer.

### `tt`

La formula è stabilita nel modello.

Nella release corrente è tipico soprattutto per query strutturali MIR. Le allocation-centric positive labels sono MAY e non forniscono `tt`.

## 6. Debug di un risultato memory

Per una cella sospetta, conservare insieme:

1. sorgente del target;
2. `annotated_icfg_v2.json`;
3. `allocation_identity.json`;
4. query esatta;
5. `entry` del grafo;
6. risultato JSON;
7. log CREMA;
8. toolchain/versione.

Controllare:

- quale `AbstractAllocId` è bindato;
- `allocation_labels` sui nodi;
- `allocation_post` se la query usa `alloc/drop/own_forg`;
- contract allocator/deallocator;
- reachability dalla `entry`;
- eventuali nodi globali non raggiungibili;
- presenza di `TOP` o family `unknown` che spiega `unk`.

## 7. Debug di query strutturali MIR

Cercare direttamente in `semantic_labels`:

```bash
grep -n 'term:return\|stmt:assign\|rvalue:checked_binary_op' \
  "$OUT/annotated_icfg_v2.json"
```

La query considera soltanto i nodi raggiungibili secondo la formula dalla entry. La presenza della stringa in un nodo globale irraggiungibile non implica necessariamente `EF ... = tt`.

## 8. Errori comuni

### `Target directories not found`

Il target non è stato compilato prima della fase CREMA/FFI.

Soluzione: eseguire `cargo build --manifest-path TARGET/Cargo.toml` prima dell'analisi.

### `Failed to run svf-driver`

Verificare:

```bash
test -x "$ROOT/SVF-example/src/svf-example"
```

e che CREMA venga eseguito con `cwd=$ROOT/crema`.

### capability missing

La query richiede informazione non dichiarata dall'artifact. Non rimuovere il `requires`; rigenerare l'artifact con il producer/profilo corretto.

### checker `rc=2`

È errore di parsing/validazione/evaluation boundary. Non registrarlo come `ff` o `unk`.

## 9. Full final112 protocol

Per la release ufficiale non usare una collezione manuale di comandi. Usare:

```bash
CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
"$ROOT/cqpl/run_all.sh"
```

Il protocollo fallisce se cambia silenziosamente census, query count, toolchain, capability, target registry o risultato dei regression gates.


## 10. Explainability corrente

Quando una query restituisce `unk`, non cercare subito una causa guardando tutto il grafo. Genera prima la dependency trace v6R sullo stesso artifact:

```bash
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
GRAPH="$OUT/annotated_icfg_v2.json"
QUERY="$ROOT/cqpl/queries_v2/leak_alloc.cqpl"
EXPLAIN="$OUT/leak_alloc.explain.json"

"$CHECKER" "$GRAPH" "$QUERY" \
  --json \
  --explain-json "$EXPLAIN" \
  --explain-max-witnesses 8
```

Leggere nell'ordine:

1. `result`;
2. `reason_frontier`;
3. `binding`;
4. `atomic_observations`;
5. `derivation`;
6. `relevant_nodes`;
7. soltanto dopo correlazioni aggiuntive nell'ICFG.

Il tutorial completo usa il target reale:

```text
a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool
```

e mostra l'intera catena target → CREMA → annotated ICFG → CQPL `unk` → `MAY_ALLOCATION` witness. Vedi [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md).

## Allocation disposition: leggere la provenance lifecycle

Dopo una run v6S, ogni `annotated_icfg_v2.json` può contenere `allocation_disposition[]`. Per una lettura umana usa `V6S_R1_GUIDE.md`; per aggregare i 105 leak `unk` usa:

```bash
python3 cqpl/scripts/analyze_allocation_disposition.py \
  --subjects "$OUT/subjects.tsv" \
  --baseline-wide "$BASE/query-matrix/query-results-wide.tsv" \
  --out "$OUT/allocation-disposition-summary.json"
```

Non interpretare `box_into_raw` come leak definitivo e non interpretare l'assenza di `may_deallocate` come prova MUST di leak. In r1 tutti i fatti sono MAY. Un record `raw_pointer_drop_noop` significa esplicitamente che `mem::drop(raw)` non ha effetto sul pointee.


## 11. Esempi reali validati della run `--explain-unk-verbose`

Questi esempi provengono da run reali sul toolchain pinned `nightly-2024-11-21`; non sono output sintetici. Il gate automatico `CQPL_MEMORY_ERROR_EXPLAINABILITY_GATE` richiede che, quando le query sotto sono `unk`, il finding appartenga alla famiglia corretta.

| Target reale | Query UNKNOWN rappresentativa | Finding | Strength | Interpretazione corretta |
|---|---|---|---|---|
| `a-memory_leaks_full_rust_literals/boxed_bool` | `leak_alloc_state` | `normal_return_open_manual_obligation` | `strong_abstract_evidence` | `Box::into_raw` lascia un obbligo manuale aperto su un witness che raggiunge normal return senza discharge modellato. |
| `a-use_after_free_full_rust_literals/boxed_bool` | `use_after_free_alloc_state` | `drop_then_use_without_reallocation` | `observational_candidate` | drop ordinato prima del use senza re-allocation; l'origine nel witness resta supportata dallo stato astratto e quindi non viene presentata come MUST proof. |
| `a-double_free_full_rust_literals/boxed_bool` | `double_free_alloc_state` | `repeated_drop_without_reallocation` | `observational_candidate` | due drop MAY ordinati senza re-allocation intermedia; possono esistere più witness candidati nello stesso artifact. |
| `a-code_c_to_rust_alloc/rust_box_direct_c_free_ub` | `allocator_mismatch_ub_v2` | `allocator_family_mismatch` | `strong_abstract_evidence` | producer `rust_global`, deallocator `c_malloc`: famiglie note e differenti. |

Esempio UAF abbreviato:

```text
QUERY: use_after_free_alloc_state
truth: unk

why unknown:
  - MAY_ALLOCATION
  - MAY_DEALLOCATION
  - MAY_USE
  - UNRESOLVED_CONTRACT
  - PATH_JOIN
  - QUERY_THREE_VALUED_PROPAGATION

supporting findings:
  kind       : drop_then_use_without_reallocation
  strength   : observational_candidate
  first drop : rust::main::bb6
  use        : rust::main::bb7
  evidence:
    - may_deallocation_observed
    - may_use_observed
    - ordered_drop_before_use
    - no_reallocation_between_events
```

Esempio allocator mismatch abbreviato:

```text
QUERY: allocator_mismatch_ub_v2
truth: unk

supporting findings:
  kind       : allocator_family_mismatch
  strength   : strong_abstract_evidence
  allocator  : rust_global
  deallocator: c_malloc
  evidence:
    - known_allocator_family
    - known_deallocator_family
    - allocator_families_differ
    - producer_certified_deallocator_contract
```

Questi esempi non autorizzano una reinterpretazione dei truth values. Il report spiega l'evidenza disponibile nel modello astratto; `unk` resta `unk`. Inoltre il nome del fixture indica l'intento del test, non un oracle esclusivo: una crate UAF può esporre anche candidate DF/allocator-mismatch sotto l'astrazione corrente. Valutare sempre `kind`, `strength`, frontier e witness della query specifica. Per gli output completi, il cross-check con le formule v2 e gli esempi leak/DF/no-finding vedi [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md).

## A3: panic/unwind lifecycle v1 (opt-in)

CREMA's A3 profile separates normal and unwind post-states for fallible MIR
terminators. It is intentionally opt-in and requires schema v2 plus the MIR-v2
profile:

```bash
cargo +nightly-2024-11-21 run --manifest-path crema/Cargo.toml -- TARGET \
  --only-icfg-annotated \
  --cqpl-schema-version 2 \
  --annotated-icfg-out annotated_icfg_v2.json \
  --allocation-identity-out allocation_identity.json \
  --mir-semantics-v2 \
  --panic-unwind-lifecycle-v1
```

The exported artifact must contain `panic_unwind_lifecycle_v1` in
`capabilities`.

For the B1.3 `aligned_box` case, analyze the admitted reproducer harness with
the A3 profile. A3 asks rustc for reachable dependency MIR and imports it when
`is_mir_available` holds, so the caller cleanup chain and the vulnerable/fixed
`aligned_box` body are represented in one ICFG. The runner fails closed if the
`realloc_with_default` dependency body is still absent:

```bash
python3 cqpl/benchmarks/rustsec_memory_safety_v1/run_b1_3_panic_unwind_case.py \
  --root "$PWD" \
  --case-id rustsec_2026_0282_aligned_box_realloc_panic
```

The runner fails before query interpretation if either dependency MIR is absent
or vulnerable and fixed annotated ICFGs are byte-identical. This gate separates
an input/materialization gap from a genuine unwind semantic gap.
