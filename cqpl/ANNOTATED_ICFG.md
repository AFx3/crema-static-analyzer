# Contratto CREMA → CQPL v6Q-r1c — `annotated_icfg_v2.json`

L'annotated ICFG è il boundary versionato fra l'analizzatore CREMA e il checker CQPL.

Principio fondamentale:

> CQPL non ricostruisce fatti da Rust/MIR/LLVM pretty text. Interroga soltanto struttura e semantica che CREMA ha già prodotto e serializzato.

## 1. Struttura top-level

```json
{
  "schema_version": 2,
  "entry": "rust::main::bb0",
  "capabilities": ["..."],
  "variables": [],
  "allocations": [],
  "nodes": []
}
```

La freeze final112 richiede su tutti i 112 grafi:

```text
allocation_contracts_v1
allocation_contracts_v2
allocation_state_v1
mir_semantic_labels_v1
mir_semantics_v2
```

## 2. Grafo di controllo

Ogni nodo ha:

```text
id
successors[]
```

Invarianti validate dal checker:

- ID nodo unici;
- `entry` esistente;
- tutti i successor esistenti;
- nessuna riscrittura implicita dei terminali.

CQPL non aggiunge self-loop ai terminali. `X` è strong. La semantica di `G` sui cammini massimali finiti usa la posizione terminale realmente esistente.

Il grafo globale può contenere nodi non raggiungibili dalla `entry`; restano disponibili per audit ma non influenzano una formula entry-rooted se nessun cammino li raggiunge.

## 3. `variables[]`: dominio `ProgramVar`

Ogni variabile ha un ID globale e language:

```json
{"id":"rust::...","language":"rust"}
{"id":"c::...","language":"c"}
```

Il dominio dei quantificatori `exists/forall` include sia Rust sia C.

## 4. `pre` e `post`: memoria astratta

Ogni nodo espone componenti alias:

```json
{
  "aliases": ["rust::x", "c::arg0"],
  "value": "ALLOC"
}
```

Una componente rappresenta variabili che condividono la stessa allocation implementativa in quel punto.

Valori:

```text
BOTTOM BOXTIMES ALLOC FREED MB IMMB MV TOP
```

Ordine usato da CREMA/CQPL:

```text
BOTTOM <= ogni valore
ALLOC  <= MB, IMMB, MV
ogni valore <= TOP
```

Gli altri elementi sono incomparabili se non coperti da queste relazioni.

Una variabile assente vale `BOTTOM`.

### Nota su `pre`

La pipeline corrente conserva uno stato convergente per nodo dopo il transformer; non dispone di una separata mappa stabile `Pi#_pre`. L'export non inventa uno stato: `pre` può essere esplicitamente vuoto. Il checker usa `post` per i MAY-state predicates e `pre ∪ post` soltanto per il lift locale delle label evento su alias.

## 5. `labels[]`: eventi program-variable

Vocabolario:

```text
alloc
drop
read
write
use
```

Esempio:

```json
{"predicate":"drop","variable":"rust::main::_3"}
```

Queste label sono sintattiche/esatte per il nodo. Su `ProgramVar`, il checker le valuta `tt/ff` dopo il lift sugli alias locali.

## 6. TaintState: volutamente fuori dal boundary CQPL

CREMA mantiene internamente un `TaintState` ausiliario MAY/provenance, ma `cqpl_export.rs` dichiara il boundary read-only e non serializza quel componente.

Quindi non esistono campi CQPL normativi `taint_src`, `taint_snk` o analoghi in schema v2.

La semantica query deriva da:

- `pre/post`;
- `labels`;
- `allocations`;
- `identity/event_identity`;
- `allocation_post`;
- `allocation_labels`;
- `semantic_labels`.

## 7. `allocations[]`: dominio `AbstractAllocId`

Schema v2 introduce identità astratte di allocazione separate dalle variabili di programma.

Record concettuale:

```json
{
  "id": "opaque-stable-id",
  "display": "diagnostic-only",
  "site": {...},
  "context": [...],
  "allocator_contract": {...}
}
```

L'`id` è la logica identity usata da `exists_alloc/forall_alloc`. `display` e DefPath di provenance non devono essere reinterpretati dal checker per dedurre semantica.

## 8. `identity` e `event_identity`

Sono relazioni MAY auditabili che associano variabili/place ad `AbstractAllocId`.

`identity` descrive la relazione post-state del nodo.

`event_identity` è il summary intra-node usato per materializzare eventi allocation-centric.

La freeze final112 ha confrontato 6894 record sidecar con le annotazioni nei grafi: 6894/6894 match.

## 9. `allocation_labels[]`

Esempio concettuale:

```json
{
  "predicate": "drop",
  "allocation": "...",
  "certainty": "may_abstract",
  "deallocator_contract": {...}
}
```

La certainty corrente è soltanto:

```text
may_abstract
```

Un singleton MAY target non viene promosso a fatto MUST. Per questo i predicati allocation-event restituiscono `unk` al match.

## 10. `allocation_post` e `allocation_state_v1`

`allocation_post` proietta lo stato MAY `ProgramVar` sull'identity delle allocazioni:

```text
Pi#_post,alloc(b)(a)
  = join { Pi#_post(b)(v) | a in MayId#_post(b)(v) }
```

Non è un nuovo lattice e non è una MUST analysis.

Le query che usano `alloc(a)`, `drop(a)`, `own_forg(a)` con `a : AbstractAllocId` devono dichiarare `allocation_state_v1`.

## 11. Contratti allocator/deallocator

Con `allocation_contracts_v1/v2`:

- `allocations[].allocator_contract` descrive la famiglia di origine;
- `allocation_labels[predicate=drop].deallocator_contract` descrive il release.

Famiglie:

```text
rust_global
c_malloc
unknown
```

v2 aggiunge proof basis strutturale sul deallocator. Il checker valida il vocabolario del basis ma non deduce family da stringhe `owner_def_path`, `allocator_def_path`, `callee_def_path`.

Vedi `capabilities/allocation_contracts_v2.md`.

## 12. `semantic_labels[]`

Con `mir_semantic_labels_v1`:

```text
stmt:<statement_category>
rvalue:<normalized_family>
term:<terminator_category>
```

Esempio:

```json
"semantic_labels": [
  "stmt:assign",
  "rvalue:use",
  "term:return"
]
```

Sono label block-level. Non codificano ordine intra-block.

Il vocabolario completo è in `LANGUAGE.md`.

## 13. `mir_semantics_v2`

La capability indica che l'export deriva dal profilo opt-in MIR-v2 CREMA.

Non promette automaticamente “precisione totale”. Una construct può essere:

- modellata precisamente;
- modellata conservativamente;
- fail-closed per una boundary non rappresentata.

Le tre crate registry final112 hanno telemetry con zero statement/rvalue/terminator `unmodeled`, ma possono comunque esistere call esterne opaque/conservative.

## 14. Validazione fail-closed del checker

Il checker rifiuta almeno:

- schema diverso da 1/2;
- capability incoerenti;
- `allocation_contracts_v2` senza v1;
- `allocation_state_v1` su schema !=2;
- ID duplicati;
- entry o successor mancanti;
- variabile/allocation referenziata ma non dichiarata;
- alias component invalida;
- `allocation_post` malformato;
- contract v2 o proof basis invalido;
- structural label con formato non normalizzato.

Query/capability mismatch è errore esplicito, mai `ff`.
