# Guida semplice all'explainability CQPL R2

Questa guida spiega **che cosa produce il layer explainability R2, come leggere ogni campo e come riprodurre un esempio completo** partendo da un target reale in `tests_and_target_repos`.

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

L'explainability è **osservazionale**: non cambia il risultato CQPL. Prima viene calcolato `ff|unk|tt`; solo dopo vengono costruiti explanation e assessment. FINAL112 R2 verifica 1344/1344 risultati con baseline `705 ff / 413 unk / 226 tt`.

## 1. Tre concetti da ricordare

### `ff`

La formula è refutata nel modello astratto. Per `ff` v6R non deve costruire un witness positivo.

### `unk`

La formula non è né provata né refutata con l'informazione disponibile. `unk` è un **risultato semantico valido**, non un errore del checker.

v6R aggiunge una `reason_frontier`: l'insieme delle cause di incertezza che compaiono sulla dependency trace effettivamente usata per spiegare quel risultato.

### `tt`

La formula è stabilita nel modello astratto. v6R richiede almeno un witness con almeno una osservazione atomica finale.

Un witness v6R è un witness **nell'annotated abstract Kripke model**. Non è automaticamente una concrete execution del programma.

### Assessment R2

Ogni risultato JSON contiene anche:

```text
subresult
direction
strength
basis
caveats
```

Per `unk`:

- `unk_true`: esiste evidence direzionale positiva, ma la query resta semanticamente UNKNOWN;
- `unk_unoriented`: l'evidence è insufficiente per scegliere una direzione;
- `strong_abstract_evidence`: proof chain astratta più forte, non concrete proof;
- `observational_candidate`: pattern MAY osservato;
- `unresolved`: nessun orientamento giustificato.

La stessa evidence usa lo stesso wire token sia nel finding sia in `assessment.basis`.

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
| `assessment` | Orientamento/strength/provenance del risultato; è ortogonale a `ff|unk|tt`. |
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
ff   705
unk  413
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

## 11. Interactive UNKNOWN report (`--explain-unk-verbose`)

For interactive diagnosis, the checker can render the same validated explanation
object used by `--explain-json` directly in the terminal:

```bash
cqpl_checker annotated_icfg_v2.json query.cqpl \
  --json \
  --explain-unk-verbose
```

The flag is conditional: it emits the human-readable report only when the CQPL
truth value is `unk`.  Under `--json`, stdout remains valid JSON; the verbose
report is written to stderr so scripts that parse stdout are not broken.

The report contains the uncertainty frontier, supporting findings, witness path,
evidence, interpretation, and atomic uncertainty witnesses.  It is presentation
only and cannot promote or demote `tt` / `ff` / `unk`.

The v2 runners expose the same flag.  UNKNOWN explanation sidecars remain
mandatory and fail-closed even when the verbose flag is not requested; the flag
only controls whether the already-validated report is also printed interactively.

## 12. Bug-specific supporting findings for UNKNOWN memory-error queries

`--explain-unk-verbose` keeps CQPL truth unchanged and renders a second,
read-only diagnostic layer.  For the official allocation-centric memory-error
families, `supporting_findings` is query-shape specific:

- leak: `normal_return_open_manual_obligation` when a producer-certified manual
  ownership handoff reaches normal return without a modeled discharge;
- use-after-free: `drop_then_use_without_reallocation` when the same
  `AbstractAllocId` has an ordered MAY drop followed by a MAY use/read/write and
  no intervening allocation event;
- double-free: `repeated_drop_without_reallocation` when two ordered MAY drops
  of the same `AbstractAllocId` occur without an intervening allocation event;
- allocator mismatch: `allocator_family_mismatch` when known producer and
  deallocator families differ; if either family is unresolved, the weaker
  `unresolved_allocator_contract_candidate` is emitted instead.

`strong_abstract_evidence` never means concrete MUST proof.  It means the
already-annotated abstract model contains the complete ordered evidence pattern
for the queried bug class.  `observational_candidate` is used when a required
contract/origin component is unresolved.  Neither strength changes the
three-valued query result.

If no bug-specific witness can be certified, the verbose report prints an
explicit interpretation stating that UNKNOWN denotes insufficient abstract
evidence for a definite verdict and is not, by itself, a positive bug finding.


## 13. Esempi empirici reali validati: leak, UAF, double-free, allocator mismatch

Questa sezione documenta output osservati su run reali con `--explain-unk-verbose`. Gli esempi sono intenzionalmente diversi per strength: la guida deve mostrare sia evidenza forte sia candidati osservazionali, senza trasformare il diagnostic layer in una proof MUST.

### 13.1 Leak: obbligo manuale aperto fino al normal return

Target:

```text
a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool
```

Output rappresentativo:

```text
QUERY: leak_alloc_state
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
```

Interpretazione: la query resta `unk` perché l'evento allocation è MAY, ma il diagnostic layer osserva un handoff `Box::into_raw` producer-certified seguito da normal return senza discharge sul witness. La documentazione Rust di `Box::into_raw` assegna al chiamante la responsabilità di distruggere e liberare la memoria; `Box::from_raw` è il meccanismo standard per ripristinare ownership RAII.

### 13.2 Use-after-free: candidato ordinato drop -> use

Target:

```text
a-code_full_rust/a-use_after_free_full_rust_literals/boxed_bool
```

Output rappresentativo:

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
  path:
    -> rust::main::bb6
    -> rust::main::bb7
  evidence:
    - allocation_state_includes_allocated
    - may_deallocation_observed
    - may_use_observed
    - ordered_drop_before_use
    - no_reallocation_between_events
```

Interpretazione: l'ordine drop-then-use è esattamente il pattern astratto richiesto dalla classe UAF, ma la strength resta `observational_candidate` perché l'origine nel witness è supportata dallo stato allocation-centric e compare una provenance contrattuale irrisolta. Questo è coerente con le API raw-pointer Rust: un read/write richiede che il puntatore sia valido per l'accesso.

### 13.3 Double-free: più witness candidati possono coesistere

Target:

```text
a-code_full_rust/a-double_free_full_rust_literals/boxed_bool
```

Uno dei witness osservati:

```text
QUERY: double_free_alloc_state
truth: unk

supporting findings:
  kind       : repeated_drop_without_reallocation
  strength   : observational_candidate
  first drop : rust::main::bb10
  second drop: rust::main::bb12
  path:
    -> rust::main::bb10
    -> rust::main::bb13
    -> rust::main::bb12
  evidence:
    - allocation_state_includes_allocated
    - may_deallocation_observed
    - two_ordered_drops_observed
    - no_reallocation_between_events
```

Nella run reale il report contiene più witness `repeated_drop_without_reallocation`. Non sono duplicati semantici da comprimere arbitrariamente: rappresentano coppie ordinate/path candidate differenti. La documentazione Rust di `Box::from_raw` avverte esplicitamente che ricostruire ownership due volte dallo stesso raw pointer può causare double-free; il finding CQPL resta comunque MAY/observational finché l'origine e le drop non sono MUST.

### 13.4 Allocator mismatch: famiglie note e diverse

Target:

```text
a-code_c_to_rust_alloc/rust_box_direct_c_free_ub
```

Output rappresentativo:

```text
QUERY: allocator_mismatch_ub_v2
truth: unk

why unknown:
  - MAY_ALLOCATION
  - MAY_DEALLOCATION
  - QUERY_THREE_VALUED_PROPAGATION

supporting findings:
  kind       : allocator_family_mismatch
  strength   : strong_abstract_evidence
  allocator  : rust_global
  deallocator: c_malloc
  evidence:
    - allocation_state_includes_allocated
    - may_deallocation_observed
    - known_allocator_family
    - known_deallocator_family
    - allocator_families_differ
    - producer_certified_deallocator_contract
```

Interpretazione: qui entrambe le famiglie sono note, quindi il diagnostic layer può classificare il mismatch come `strong_abstract_evidence`. La truth resta `unk` perché la relazione allocation/deallocation è MAY. Questo è coerente con il contratto Rust degli allocator: `Allocator::deallocate` richiede un blocco attualmente allocato tramite quello stesso allocator; `Box::from_raw` richiede a sua volta memoria compatibile con l'allocator/layout di `Box`.

### 13.5 Contratto irrisolto: non chiamarlo mismatch provato

In altri target reali può comparire:

```text
kind       : unresolved_allocator_contract_candidate
strength   : observational_candidate
allocator  : rust_global
deallocator: unknown
```

Questo output è intenzionalmente più debole. `unknown` non è una famiglia diversa: significa che il producer non ha certificato abbastanza informazione per confrontare le famiglie. Il report deve quindi dire "candidate", non "allocator mismatch confirmed".

### 13.6 UNKNOWN senza finding positivo

È corretto anche questo caso:

```text
supporting findings:
  <none>

interpretation:
  No bug-specific supporting witness was certified beyond the uncertainty frontier.
  UNKNOWN therefore means insufficient abstract evidence for a definite verdict,
  not a positive finding by itself.
```

Questa uscita è essenziale per evitare che il solo fatto di avere `unk` venga reinterpretato come vulnerabilità.

### 13.7 Gate empirico

La run real-target usata per questi esempi deve chiudere con:

```text
UAF
  use_after_free_alloc       -> drop_then_use_without_reallocation
  use_after_free_alloc_state -> drop_then_use_without_reallocation

DOUBLE_FREE
  double_free_alloc       -> repeated_drop_without_reallocation
  double_free_alloc_state -> repeated_drop_without_reallocation

ALLOCATOR_MISMATCH
  allocator_mismatch_ub    -> allocator_family_mismatch
  allocator_mismatch_ub_v2 -> allocator_family_mismatch

errors = 0
CQPL_MEMORY_ERROR_EXPLAINABILITY_GATE: PASS
```

Il gate verifica la coerenza fra famiglia della query e famiglia del finding; non richiede che ogni query memory sia `unk` e non promuove il result a `tt`.


### 13.8 Cross-check con le formule ufficiali v2 e con i contratti Rust

I finding UAF e double-free non sono euristiche aggiunte dopo la query: riprendono la stessa relazione d'ordine già richiesta dalle formule v2.

```cqpl
# use_after_free_alloc(.state), forma essenziale
drop_l(a) && EX E[(!alloc_l(a)) U use_l(a)]

# double_free_alloc(.state), forma essenziale
drop_l(a) && EX E[(!alloc_l(a)) U drop_l(a)]
```

Perciò `drop_then_use_without_reallocation` e `repeated_drop_without_reallocation` sono diagnostiche strutturalmente allineate alla formula, ma mantengono la stessa natura MAY degli atomi. Per allocator mismatch il finding non ricostruisce una nuova nozione di UB: espone la provenance delle famiglie che alimenta `allocator_mismatch_l(a)`.

Il leak finding è deliberatamente più debole della formula temporale completa: `normal_return_open_manual_obligation` documenta un witness ownership/disposition rilevante, ma non viene usato come sostituto di `EX EG !drop(a)`. È per questo che può coesistere con `query_truth = unk`.

Riferimenti upstream Rust usati per controllare la coerenza del diagnostic layer:

- [`Box::into_raw` / `Box::from_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html): dopo `into_raw` il chiamante assume la responsabilità della memoria; un uso scorretto di `from_raw` può causare double-free;
- [raw pointers](https://doc.rust-lang.org/std/primitive.pointer.html) e [`ptr::read`](https://doc.rust-lang.org/std/ptr/fn.read.html): un accesso raw richiede memoria valida per l'accesso;
- [`Allocator::deallocate`](https://doc.rust-lang.org/std/alloc/trait.Allocator.html): il blocco passato a `deallocate` deve essere attualmente allocato tramite quell'allocator.

Questi riferimenti giustificano la classificazione delle evidenze, non trasformano una relazione MAY del modello CQPL in una prova concreta MUST.
