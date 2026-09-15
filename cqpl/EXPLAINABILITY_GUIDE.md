# Guida semplice all'explainability CQPL v6R-r1

Questa guida spiega **che cosa produce v6R-r1, come leggere ogni campo e come riprodurre un esempio completo** partendo da un target reale in `tests_and_target_repos`.

L'idea centrale è semplice:

```text
programma Rust/C
    |
    v
  CREMA
    |
    | annotated_icfg_v2.json
    v
  CQPL
    |
    | ff | unk | tt
    v
explainability v6R
    |
    +-- perché il risultato è unk?
    +-- quale binding astratto è stato usato?
    +-- quali nodi e atomi hanno contribuito?
    +-- se il risultato è tt, qual è il witness nel modello astratto?
```

L'explainability è **osservazionale**: non cambia il risultato CQPL. Prima viene calcolato il normale `ff|unk|tt`; solo dopo viene costruita la spiegazione. La validation v6R-r1 ha verificato 1344/1344 risultati identici alla baseline v6Q-r1c.

## 1. Tre concetti da ricordare

### `ff`

La formula è refutata nel modello astratto. Per `ff` v6R non deve costruire un witness positivo.

### `unk`

La formula non è né provata né refutata con l'informazione disponibile. `unk` è un **risultato semantico valido**, non un errore del checker.

v6R aggiunge una `reason_frontier`: l'insieme delle cause di incertezza che compaiono sulla dependency trace effettivamente usata per spiegare quel risultato.

### `tt`

La formula è stabilita nel modello astratto. v6R richiede almeno un witness con almeno una osservazione atomica finale.

Un witness v6R è un witness **nell'annotated abstract Kripke model**. Non è automaticamente una concrete execution del programma.

## 2. Il file `.explain.json`

Esempio minimo:

```json
{
  "schema": "cqpl_explanation_v1",
  "taxonomy": "cqpl_uncertainty_reasons_v1",
  "result": "unk",
  "entry": "rust::main::bb0",
  "reason_frontier": [
    "MAY_ALLOCATION",
    "QUERY_THREE_VALUED_PROPAGATION"
  ],
  "witnesses": [
    {
      "truth": "unk",
      "binding": {
        "a": {
          "kind": "allocation",
          "value": "...AbstractAllocId..."
        }
      },
      "relevant_nodes": ["rust::main::bb0"],
      "complete_dependency_trace": true
    }
  ]
}
```

## 3. Significato di ogni campo top-level

| Campo | Significato semplice |
|---|---|
| `schema` | Versione del formato JSON di explainability. In v6R-r1 è `cqpl_explanation_v1`. |
| `taxonomy` | Versione del vocabolario delle cause di incertezza. In v6R-r1 è `cqpl_uncertainty_reasons_v1`. |
| `result` | Lo stesso risultato CQPL normale: `ff`, `unk` oppure `tt`. |
| `entry` | Nodo di ingresso del Kripke già proiettato su cui è stata valutata la query. |
| `scope_note` | Ricorda che la spiegazione riguarda il modello astratto, non una esecuzione concreta garantita. |
| `reason_frontier` | Insieme deduplicato delle cause di `unk` incontrate sulla dependency trace dei witness emessi. |
| `reason_counts` | Quante volte ogni reason code compare nei witness emessi. Non è una probabilità. |
| `witnesses` | Una o più spiegazioni strutturate del risultato `unk`/`tt`, fino al limite richiesto. |
| `diagnostics` | Gate interni che verificano che la spiegazione non sia vuota o tautologica. |

`reason_frontier` è diverso da “tutte le imprecisioni presenti nel grafo”: include solo ragioni osservate sulla derivazione selezionata.

## 4. Significato dei campi di un witness

Ogni elemento di `witnesses` contiene:

| Campo | Significato |
|---|---|
| `truth` | Valore della derivazione spiegata: normalmente `unk` o `tt`. |
| `binding` | Valori assegnati alle variabili logiche della query, per esempio `a : AbstractAllocId`. |
| `relevant_nodes` | Nodi del Kripke attraversati/consultati dalla dependency trace. |
| `reasons` | Reason code presenti in quel witness. |
| `derivation` | Sequenza di passi logici/temporali che porta dalla formula root agli atomi rilevanti. |
| `atomic_observations` | Gli atomi CQPL finali realmente osservati, con nodo, truth e cause specifiche. |
| `complete_dependency_trace` | `true` se la trace emessa è completa per la dipendenza esistenziale selezionata; per spiegazioni universali può essere `false` perché una proof tree completa può essere molto grande. |

### `binding`

Esempio:

```json
"binding": {
  "a": {
    "kind": "allocation",
    "value": "{...}"
  }
}
```

Significa: la variabile logica `a` della query è stata bindata a uno specifico `AbstractAllocId` serializzato. `value` è un identificatore astratto opaco, non un indirizzo runtime.

### `derivation`

Esempio:

```json
[
  {"node":"rust::main::bb0","formula_kind":"exists_allocation","truth":"unk"},
  {"node":"rust::main::bb0","formula_kind":"eventually","truth":"unk"},
  {"node":"rust::main::bb0","formula_kind":"and","truth":"unk"},
  {"node":"rust::main::bb0","formula_kind":"event_atom","truth":"unk"}
]
```

Si legge dall'alto verso il basso: il quantificatore esistenziale è `unk` perché una sua istanza conduce a un `EF` `unk`; questo porta alla congiunzione e infine all'atomo che contiene l'origine concreta dell'incertezza.

### `atomic_observations`

Esempio:

```json
{
  "node": "rust::main::bb0",
  "atom": "alloc_l(a)",
  "truth": "unk",
  "reasons": ["MAY_ALLOCATION"],
  "detail": {
    "matching_allocation_labels": "1",
    "predicate": "alloc_l"
  }
}
```

Questo è il punto più importante per capire un `unk`: esiste una label di allocazione compatibile, ma su `AbstractAllocId` schema-v2 la sua certezza è `may_abstract`; quindi l'atomo vale `unk`, non `tt`.

## 5. I reason code v1

| Reason | Significato pratico |
|---|---|
| `MAY_ALLOCATION` | L'allocazione rilevante è conosciuta solo come MAY. |
| `MAY_DEALLOCATION` | Drop/deallocazione rilevante è MAY. |
| `MAY_USE` | Read/write/use rilevante è MAY. |
| `MAY_OWNERSHIP` | Informazione di ownership/forget è MAY. |
| `ALIAS_JOIN` | Il program variable appartiene a una componente alias con più elementi. |
| `ABSTRACT_COMPONENT_MERGE` | Un identity record rilevante può riferirsi a più abstract allocations. |
| `ABSTRACT_TOP_STATE` | La cella di stato rilevante è `TOP`, quindi troppo imprecisa. |
| `UNRESOLVED_CONTRACT` | Il contratto allocator/deallocator necessario non è risolto. |
| `PATH_JOIN` | L'incertezza attraversa un join/fixpoint con successori di valori diversi. |
| `QUERY_THREE_VALUED_PROPAGATION` | Un `unk` atomico è propagato da operatori logici, quantificatori o CTL. |

Questi codici esistono nella tassonomia ma **v6R-r1 non li inventa** senza provenance producer-certified:

```text
EXTERNAL_EFFECT
UNRESOLVED_ESCAPE
GLOBAL_TOP_EFFECT
CONTROL_FLOW_UNRESOLVED
HIGHER_ORDER_UNRESOLVED
```

Per esempio, una cella `TOP` non viene automaticamente chiamata `GLOBAL_TOP_EFFECT`.

## 6. I campi `diagnostics`

| Campo | Interpretazione |
|---|---|
| `witnesses_requested` | Limite massimo richiesto con `--explain-max-witnesses`. |
| `witnesses_emitted` | Numero realmente emesso. |
| `unknown_has_reason_frontier` | Gate: se `result=unk`, la frontiera non può essere vuota. Per risultati diversi da `unk` il gate è vero per non-applicabilità. |
| `unknown_has_specific_origin` | Gate: un `unk` deve avere almeno una causa atomica specifica; `PATH_JOIN` o `QUERY_THREE_VALUED_PROPAGATION` da soli non bastano. |
| `true_has_witness` | Gate: se `result=tt`, deve esistere un witness. Per `ff/unk` è vero per non-applicabilità. |
| `true_has_atomic_witness` | Gate: se `result=tt`, almeno un witness deve terminare in una osservazione atomica. |
| `producer_provenance_capability_present` | Indica se l'artifact dichiara la futura capability `uncertainty_provenance_v1`. Nei grafi v6Q-r1c congelati è `false`. |
| `reserved_reason_codes_not_inferred` | Elenco dei reason code che l'explainer ha deliberatamente evitato di dedurre senza provenance certificata. |

## 7. Esempio completo: `boxed_bool__ml`

Useremo il target:

```text
tests_and_target_repos/
  a-code_full_rust/
    a-memory_leaks_full_rust_literals/
      boxed_bool/
```

Nel final112 source audit questo soggetto è `boxed_bool__ml`, classe reviewed `ML`. L'audit osserva nel sorgente una `Box` allocation, un `Box::into_raw`, nessun `Box::from_raw` e nessun `free`. È quindi un esempio didattico piccolo del pattern “ownership resa raw e non recuperata”.

Il codice esatto del target resta la fonte normativa; schematicamente il pattern è:

```rust
let b = Box::new(true);
let raw = Box::into_raw(b);
// raw non viene ricostruito in un Box e non viene deallocato
```

Lo snippet sopra è **didattico**, non una trascrizione byte-per-byte del file sorgente.

### Passo 1 — impostare ambiente

Dal repository installato:

```bash
set +e
ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21
cd "$ROOT"
```

Verifica:

```bash
rustup run "$NIGHTLY" rustc --version
```

Atteso:

```text
rustc 1.84.0-nightly (3fee0f12e 2024-11-20)
```

### Passo 2 — eseguire CREMA e le 12 query sul singolo target

Usiamo il runner canonico, che compila il target, esegue CREMA e poi CQPL:

```bash
OUT="$ROOT/repro-results/v6r-tutorial-boxed-bool-ml"
rm -rf "$OUT"

python3 "$ROOT/cqpl/scripts/run_one_target_v6q_r1c.py" \
  --root "$ROOT" \
  --relative-path 'a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool' \
  --out "$OUT" \
  --toolchain "$NIGHTLY"

RC_ONE=$?
echo "one_target_rc=$RC_ONE"
```

Atteso:

```text
CQPL_ONE_TARGET v6Q-r1c: PASS queries=12 ...
one_target_rc=0
```

I file principali sono:

```text
$OUT/annotated_icfg_v2.json     grafo annotato prodotto da CREMA
$OUT/allocation_identity.json   sidecar identity
$OUT/queries/*.json             risultati ordinari CQPL
$OUT/query-results.tsv          matrice delle 12 query
```

### Passo 3 — osservare il risultato CQPL ordinario

```bash
cat "$OUT/queries/leak_alloc.json"
```

Per questo soggetto la baseline congelata è:

```text
result = unk
```

`unk` **non** significa “nessun leak” e non significa “leak provato”. Significa che il modello MAY non permette una conclusione booleana più forte.

### Passo 4 — eseguire la stessa query con explainability

Costruiamo il checker v6R se necessario:

```bash
cargo +"$NIGHTLY" build \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml"
```

Poi eseguiamo la **stessa** query sullo **stesso** artifact:

```bash
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
QUERY="$ROOT/cqpl/queries_v2/leak_alloc.cqpl"
EXPLAIN="$OUT/leak_alloc.explain.json"

"$CHECKER" \
  "$OUT/annotated_icfg_v2.json" \
  "$QUERY" \
  --json \
  --explain-json "$EXPLAIN" \
  --explain-max-witnesses 8 \
  > "$OUT/leak_alloc.v6r.result.json"

RC_EXPLAIN=$?
echo "explain_rc=$RC_EXPLAIN"
```

Atteso:

```text
explain_rc=0
```

La regola fondamentale è:

```text
$OUT/queries/leak_alloc.json       -> result=unk
$OUT/leak_alloc.v6r.result.json    -> result=unk
$OUT/leak_alloc.explain.json       -> result=unk + spiegazione
```

v6R aggiunge informazione diagnostica ma non cambia la truth.

### Passo 5 — leggere solo i campi importanti

```bash
python3 - "$EXPLAIN" <<'PY'
import json, sys
x = json.load(open(sys.argv[1]))

print("result:", x["result"])
print("entry:", x["entry"])
print("reason_frontier:", x["reason_frontier"])

for i, w in enumerate(x["witnesses"]):
    print("\nwitness", i)
    print(" truth:", w["truth"])
    print(" binding:", w["binding"])
    print(" relevant_nodes:", w["relevant_nodes"])
    print(" complete_dependency_trace:", w["complete_dependency_trace"])
    for atom in w["atomic_observations"]:
        print(" atom:", atom["atom"])
        print(" atom truth:", atom["truth"])
        print(" atom reasons:", atom.get("reasons", []))
PY
```

Nel run v6R-r1 validato, `boxed_bool__ml/leak_alloc` produce:

```text
result: unk
entry: rust::main::bb0
reason_frontier:
  MAY_ALLOCATION
  QUERY_THREE_VALUED_PROPAGATION
```

Il binding `a` identifica l'abstract allocation site con callee:

```text
std::boxed::Box::<bool>::new
```

L'osservazione atomica decisiva è:

```text
node   = rust::main::bb0
atom   = alloc_l(a)
truth  = unk
reason = MAY_ALLOCATION
matching_allocation_labels = 1
```

### Passo 6 — interpretare la derivazione

Per questo esempio la dependency trace validata è, in forma semplificata:

```text
exists_allocation(a) = unk
        |
        v
EF(...) = unk
        |
        v
AND = unk
        |
        v
alloc_l(a) = unk
        |
        v
MAY_ALLOCATION
```

Questo ci dice **dove nasce l'incertezza osservata**: la query trova un allocation candidate, ma l'evento allocation-centric su `AbstractAllocId` è MAY.

Non ci autorizza invece a dire:

```text
"il leak è causato da libc"
"il leak è causato da aliasing"
"il programma ha sicuramente un leak concreto perché CQPL ha dato unk"
```

Nessuna di queste conclusioni è contenuta nel witness.

### Passo 7 — confronto con `leak_alloc_state`

Puoi ripetere:

```bash
QUERY="$ROOT/cqpl/queries_v2/leak_alloc_state.cqpl"
EXPLAIN_STATE="$OUT/leak_alloc_state.explain.json"

"$CHECKER" \
  "$OUT/annotated_icfg_v2.json" \
  "$QUERY" \
  --json \
  --explain-json "$EXPLAIN_STATE" \
  > "$OUT/leak_alloc_state.v6r.result.json"
```

La query state-based può aggiungere ragioni come `ABSTRACT_TOP_STATE` o `MAY_DEALLOCATION`, perché interroga anche `allocation_post`. Questo permette di distinguere imprecisione event-centric da imprecisione dello stato astratto.

## 8. Come passare dal singolo esempio alla matrice 112 x 12

Il protocollo validato v6R esegue lo stesso meccanismo su tutti i 112 soggetti e tutte le 12 query:

```text
112 subjects x 12 queries = 1344 explanations
```

Il run congelato v6R-r1 ha verificato:

```text
baseline result mismatches          0 / 1344
unknown without reason frontier     0
unknown without specific origin     0
true without witness                0
true without atomic witness         0

result counts:
ff   650
unk  468
tt   226
```

Per le due query leak:

```text
leak_alloc       unk = 105 / 112
leak_alloc_state unk = 105 / 112
MAY_ALLOCATION nella frontier = 105 / 105 per entrambe
```

## 9. Cosa fare quando trovi un `unk`

Usa questo ordine:

1. guarda `reason_frontier`;
2. guarda il `binding` dell'allocation/program variable;
3. guarda `atomic_observations`;
4. segui `derivation` e `relevant_nodes`;
5. apri gli stessi nodi in `annotated_icfg_v2.json`;
6. solo dopo cerca correlazioni aggiuntive nel grafo;
7. non trasformare una correlazione in una causa se non appare nella dependency trace.

## 10. Cosa v6R-r1 non spiega ancora

I grafi v6Q-r1c non esportano provenance sufficiente per attribuire con certezza:

- effetti di funzioni esterne;
- escape di ownership;
- origine precisa di un `GLOBAL_TOP`;
- alcuni unresolved control-flow/higher-order effects.

Per questo i relativi reason code sono riservati e non inferiti. Questa è una scelta conservativa, non una mancanza da aggirare con euristiche sui nomi delle funzioni.

Per il contratto completo vedi [EXPLAINABILITY.md](EXPLAINABILITY.md). Per la semantica CQPL vedi [LANGUAGE.md](LANGUAGE.md).
