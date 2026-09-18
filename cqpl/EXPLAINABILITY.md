# CQPL R2 — contratto di explainability e assessment

## 1. Scopo

Il layer explainability è **osservazionale rispetto alla truth semantics**:

```text
result = ff | unk | tt
```

CQPL valuta prima la query sul Kripke. Solo dopo costruisce la spiegazione usando lo stesso modello e la stessa valuation.

R2 aggiunge un assessment ortogonale:

```text
subresult = tt | ff | unk_true | unk_false | unk_mixed | unk_unoriented
direction = true | false | mixed | none
strength  = abstract_established | strong_abstract_evidence
          | observational_candidate | unresolved
```

`unk_true` non è un quarto truth value: resta `result=unk`. Indica soltanto che esiste una proof chain astratta direzionale a favore del pattern cercato.

`unk_false` e `unk_mixed` restano riservati finché non esiste una pipeline refutante duale esplicita.

## 2. Acceptance corrente

FINAL112 R2-R1.1 ha verificato:

```text
112 subjects
12 queries
1344 results

ff  = 705
unk = 413
tt  = 226

unk_true       = 273
unk_unoriented = 140

strong_abstract_evidence = 73
observational_candidate  = 200
unresolved               = 140
```

Ogni UNKNOWN ha explanation e assessment schema-validi. La micro-release R2-R1.2 non deve cambiare nessuno di questi conteggi.

Il campo `basis` deve essere canonicale e proof-carrying:

- il token dell'evidence deve coincidere con il wire token JSON;
- `pta_basis` è positive supporting evidence solo con `svf_may_points_to` non vuoto;
- la provenance primaria storica può essere mantenuta e affiancata da `corroborating_bases` indipendenti.

Per una vista end-to-end vedere [ANALYSIS_PIPELINE.md](ANALYSIS_PIPELINE.md).

## 3. Come attivare l'explainability

```bash
cqpl_checker GRAPH QUERY \
  --json \
  --explain-json explanation.json \
  --explain-max-witnesses 8
```

Il normale stdout mantiene il risultato CQPL. La spiegazione viene scritta separatamente in `explanation.json`.

## 4. Schema `cqpl_explanation_v1`

Campi top-level:

| Campo | Contratto |
|---|---|
| `schema` | Deve essere `cqpl_explanation_v1`. |
| `taxonomy` | Deve essere `cqpl_uncertainty_reasons_v1`. |
| `result` | `ff`, `unk` o `tt`; deve coincidere col risultato ordinario. |
| `assessment` | `cqpl_result_assessment_v1`: subresult, direction, strength, basis e caveats; non modifica `result`. |
| `entry` | Entry del Kripke già proiettato. |
| `scope_note` | Specifica che la spiegazione riguarda il modello astratto. |
| `reason_frontier` | Reason code deduplicati nei witness emessi. |
| `reason_counts` | Conteggi dei reason code nei witness; non sono probabilità né classi disgiunte. |
| `supporting_findings` | Evidenza bug-supporting osservazionale separata dalla truth semantics; può essere presente anche con `result=unk`. |
| `witnesses` | Dependency trace strutturate per `unk`/`tt`. |
| `diagnostics` | Gate automatici di completezza/minima specificità. |

Per una lettura didattica campo-per-campo vedi [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md).

## 5. Witness

Un witness contiene:

```text
truth
binding
relevant_nodes
reasons
derivation
atomic_observations
complete_dependency_trace
```

### `binding`

È l'ambiente delle variabili logiche. I due kind possibili sono:

```text
program_var
allocation
```

Per `allocation`, `value` serializza un `AbstractAllocId`. Non è un indirizzo runtime.

### `relevant_nodes`

Sono i nodi del Kripke usati dalla dependency trace. Non sono necessariamente tutti i nodi di un percorso concreto.

### `derivation`

Descrive gli operatori attraversati, per esempio:

```text
exists_allocation -> eventually -> and -> event_atom
```

Ogni passo registra `node`, `formula_kind`, `truth` e, se utile, `detail`.

### `atomic_observations`

Sono gli endpoint della spiegazione, per esempio:

```text
alloc_l(a) = unk  reason=MAY_ALLOCATION
term_l(return) = tt
```

Per un risultato `tt`, almeno un witness deve avere almeno una atomic observation.

### `complete_dependency_trace`

Per il frammento CTL esistenziale usato dalle query memory ufficiali, la trace selezionata è completa e il campo è `true`.

Per formule universali, l'explainer può emettere una trace rappresentativa con `false`, perché una proof tree universale completa può essere molto grande.

Questo campo non afferma mai che il witness sia una concreta esecuzione del programma.

## 6. Tassonomia stabile `cqpl_uncertainty_reasons_v1`

| Code | Significato | Evidenza |
|---|---|---|
| `MAY_ALLOCATION` | allocation fact rilevante è MAY | allocation label/state |
| `MAY_DEALLOCATION` | drop/deallocation rilevante è MAY | allocation label/state |
| `MAY_USE` | read/write/use rilevante è MAY | allocation label |
| `MAY_OWNERSHIP` | ownership/forget state è MAY | abstract state |
| `ALIAS_JOIN` | program variable in alias component >1 | pre/post state |
| `ABSTRACT_COMPONENT_MERGE` | identity rilevante mappa a più abstract allocations | identity/event identity |
| `ABSTRACT_TOP_STATE` | cella di stato rilevante è `TOP` | serialized state |
| `UNRESOLVED_CONTRACT` | contract allocator/deallocator rilevante non risolto | contract metadata |
| `PATH_JOIN` | incertezza propagata attraverso join/fixpoint | model-checking derivation |
| `QUERY_THREE_VALUED_PROPAGATION` | `unk` propagato da logica/quantificatore/CTL | model-checking derivation |

Codici riservati ma non inferiti dai grafi v6Q-r1c:

```text
EXTERNAL_EFFECT
UNRESOLVED_ESCAPE
GLOBAL_TOP_EFFECT
CONTROL_FLOW_UNRESOLVED
HIGHER_ORDER_UNRESOLVED
```

Servono provenance producer-certified. v6R-r1 non deduce `EXTERNAL_EFFECT` da un nome di funzione e non deduce `GLOBAL_TOP_EFFECT` dalla sola presenza di `TOP`.

## 7. Gate diagnostici

`diagnostics` contiene:

```text
witnesses_requested
witnesses_emitted
unknown_has_reason_frontier
unknown_has_specific_origin
true_has_witness
true_has_atomic_witness
producer_provenance_capability_present
reserved_reason_codes_not_inferred
```

Le quattro proprietà centrali sono condizionali:

- se `result=unk`, `unknown_has_reason_frontier` deve essere true;
- se `result=unk`, `unknown_has_specific_origin` deve essere true;
- se `result=tt`, `true_has_witness` deve essere true;
- se `result=tt`, `true_has_atomic_witness` deve essere true.

Per risultati a cui la condizione non si applica, il relativo gate è true per non-applicabilità.

`PATH_JOIN` e `QUERY_THREE_VALUED_PROPAGATION` da soli **non** sono una origine specifica sufficiente per un `unk`.

## 8. Leak: risultato misurato

Query event-based congelata:

```cqpl
exists_alloc a. EF (alloc_l(a) && EX EG !drop_l(a))
```

Query state-based congelata:

```cqpl
exists_alloc a. EF (alloc(a) && EX EG !drop(a))
```

In schema v2 un positivo `alloc_l(a)` su `AbstractAllocId` è `may_abstract`; anche `alloc(a)` in `allocation_state_v1` è MAY. Quindi un candidate positivo non diventa automaticamente `tt`.

La validation v6R-r1 ha misurato:

```text
leak_alloc:
  unk = 105 / 112
  MAY_ALLOCATION frontier = 105 / 105
  ABSTRACT_COMPONENT_MERGE = 8 / 105
  MAY_DEALLOCATION = 13 / 105
  PATH_JOIN = 15 / 105
  UNRESOLVED_CONTRACT = 5 / 105

leak_alloc_state:
  unk = 105 / 112
  MAY_ALLOCATION frontier = 105 / 105
  ABSTRACT_TOP_STATE = 89 / 105
  MAY_DEALLOCATION = 89 / 105
  ABSTRACT_COMPONENT_MERGE = 8 / 105
  PATH_JOIN = 8 / 105
```

I conteggi `reason_presence` si sovrappongono e **non devono essere sommati**. `signature_counts` è invece la partizione per frontier completa osservata.

## 9. Cosa possiamo e non possiamo concludere

Possiamo dire:

> “Questa query è `unk` perché la dependency trace contiene un `alloc_l(a)=unk` con reason `MAY_ALLOCATION`.”

Non possiamo dire automaticamente:

> “La vulnerabilità è causata da MAY_ALLOCATION.”

Il primo è un fatto sulla derivazione della query. Il secondo sarebbe un claim sul programma concreto e richiede ulteriore semantica/oracle.

Analogamente, ridurre il numero di `unk` non è di per sé un miglioramento se introduce falsi `ff` sui reviewed positive o falsi `tt` sui reviewed clean.

## 10. Metriche di precisione

Usare:

```text
unknown_rate
vulnerable_non_refutation_rate = (tt + unk) / reviewed_positive
vulnerable_definite_detection = tt / reviewed_positive
clean_definite_refutation = ff / reviewed_negative
unexpected_ff_on_reviewed_positive
unexpected_tt_on_reviewed_clean
```

Gate di regressione principale:

```text
unexpected_ff_on_reviewed_positive == 0
```

La baseline validata mantiene:

```text
ML     33/33 non-refuted
DF     29/29 non-refuted
UAF    22/22 non-refuted
UB_FFI 18/18 non-refuted
```

## 11. Stato e limiti

v6R-r1 è uno strumento di misura, non un intervento di precisione. Non aggiunge:

- MUST allocation/deallocation;
- escape analysis;
- external/libc summaries;
- nuovo abstract domain;
- nuova query leak.

Questi interventi appartengono alle versioni successive e devono essere scelti dai dati misurati.

Per l'esempio completo target → CREMA → CQPL → explanation, vedi [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md).

## 12. Invariante di orchestrazione: ogni `unk` v2 ha un report

Per le esecuzioni standard schema-v2, explainability non è più un pass manuale opzionale.
I runner ufficiali applicano la policy fail-closed:

```text
query result = ff  -> nessun report obbligatorio
query result = tt  -> nessun report obbligatorio
query result = unk -> explanation sidecar obbligatorio
```

L'obbligo vale per ogni query v2 effettivamente eseguita e per ogni target/crate.
Il numero corrente del catalogo ufficiale è 12 query; la policy non dipende dal
numero e resta valida per un sottoinsieme selezionato da un runner specializzato.

Il runner esegue prima la query normale. Solo se il risultato congelato è `unk`,
riesegue la stessa coppia artifact/query con `--explain-json`. La seconda esecuzione
deve soddisfare tutte le proprietà seguenti:

```text
plain_result == unk
explanation.result == unk
reason_frontier != []
diagnostics.unknown_has_reason_frontier == true
diagnostics.unknown_has_specific_origin == true
```

Qualunque violazione è un errore infrastrutturale: il runner termina non-zero.
Questa policy non modifica CTL, il lattice three-valued o il risultato della query.

Per il one-target runner i sidecar sono:

```text
queries/<query>.explain.json
unknown-explanations.tsv
unknown-explanations-summary.json
```

Per la matrice multi-target:

```text
results/<target>/<query>.explain.json
unknown-explanations.tsv
unknown-explanations-summary.json
```

`unknown-explanations-summary.json` usa lo schema:

```text
cqpl_unknown_explanations_v1
```

e certifica l'invariante quantitativo:

```text
unknown_results == explanations_generated
complete == true
```

`supporting_findings` non è obbligatorio per ogni `unk`: dipende dalla disponibilità
di evidenza diagnostica specifica per la proprietà. In particolare, una leak query
può associare a `unk` un `normal_return_open_manual_obligation` con
`strong_abstract_evidence`, mentre un altro `unk` può avere solo una frontier
d'incertezza specifica. L'assenza di un supporting finding non cambia la truth
semantics e non autorizza a inventare una diagnosi positiva.

### v6T memory-error diagnostic findings

The v6T interactive UNKNOWN layer generalizes `supporting_findings` beyond
leaks.  Query truth remains the frozen CQPL `tt/ff/unk`; the diagnostic layer
only summarizes ordered evidence already present in the projected Kripke
model.  UAF findings require drop-before-use with no re-allocation, double-free
findings require two ordered drops with no re-allocation, and allocator-mismatch
findings report the serialized allocator/deallocator families.  Unknown
allocator families are reported as unresolved candidates rather than as proven
family mismatches.
