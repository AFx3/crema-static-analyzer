# Guida operativa — analizzare target e crate con CREMA + CQPL v6R-r1

Questa guida descrive la procedura riproducibile usata dalla release final112.

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
  --relative-path 'a-code_full_rust/a-double_free_full_rust_literals/boxed_bool'
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
  --json
```

### 3.7 Eseguire tutte le query

Usare `scripts/run_one_target_v6q_r1c.py` oppure iterare i 12 file. Il protocollo finale tratta `rc != 0` come errore, non come truth value.

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

Per query memory è normale e frequente. Investigare sorgente, identity, allocation labels e percorso, ma non promuovere `unk` a bug concreto senza ulteriore evidenza.

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


## 10. Explainability v6R-r1

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
