# CQPL — query language e model checker per CREMA

CQPL valuta proprietà temporali sull'**annotated ICFG** prodotto da CREMA.

Il checker CQPL non legge direttamente Rust, MIR, LLVM o C: riceve un JSON già
annotato da CREMA e una query `.cqpl`.

```text
Cargo project (Rust/C) -> CREMA -> annotated ICFG -> CQPL -> ff | unk | tt
```

## Risultati

- `ff`: il modello astratto refuta il pattern;
- `unk`: il modello non può refutare né stabilire il pattern;
- `tt`: la formula è stabilita nel modello astratto.

`unk` è normale quando la proprietà dipende da informazione MAY. `tt` non è,
da solo, una prova di una concreta esecuzione del programma: l'ICFG e le
relazioni di identità/evento restano astrazioni.

## Stato corrente: v6N-r1a

Il profilo end-to-end corrente usa **schema v2** con entrambe le capability:

```text
allocation_state_v1
allocation_contracts_v2
```

`allocation_contracts_v2` è un refinement fail-closed di
`allocation_contracts_v1`; un artifact che dichiara v2 deve dichiarare anche v1.

La semantica three-valued di `allocator_mismatch_l` non è stata modificata da
v6M-r1c: v6N-r1a aumenta la precisione dei contract prodotti da CREMA.

## Query ufficiali correnti

Per gli esperimenti end-to-end v6N-r1a usare `queries_v2/`:

```text
queries_v2/leak_alloc_state.cqpl
queries_v2/double_free_alloc_state.cqpl
queries_v2/use_after_free_alloc_state.cqpl
queries_v2/allocator_mismatch_ub_v2.cqpl
```

La query:

```text
queries_v2/allocator_mismatch_ub.cqpl
```

è mantenuta per compatibilità/baseline `allocation_contracts_v1` e non è la
query allocator-mismatch ufficiale del profilo corrente v6N-r1a.

Le query in `queries/` restano il core/legacy variable-centric e servono anche
per riproducibilità storica.

## Struttura di una query

Un file `.cqpl` contiene zero o più dichiarazioni `requires ...;` seguite da una
formula. Sono supportati commenti `#` e `//`.

La query allocator mismatch corrente è:

```cqpl
requires allocation_contracts_v2;

exists_alloc a. EF (
  alloc_l(a) &&
  EX EF allocator_mismatch_l(a)
)
```

## Variabili e quantificatori

Variabili di programma:

```cqpl
exists x. phi
forall x. phi
```

`x` varia sui `ProgramVar` esportati da CREMA, incluse variabili Rust e C.

Allocazioni astratte, schema v2:

```cqpl
exists_alloc a. phi
forall_alloc a. phi
```

`a` varia sugli `AbstractAllocId` esportati da CREMA.

## Predicati supportati

### Predicati semantici MAY

| Predicato | Significato |
|---|---|
| `alloc(x)` | `x` può rappresentare memoria allocata |
| `drop(x)` | `x` può rappresentare memoria freed/deallocata |
| `own_forg(x)` | l'ownership può essere stata abbandonata |

Nelle query allocation-centric v6M/v6N, `alloc(a)` e `drop(a)` possono essere
valutati anche su un `AbstractAllocId` tramite `allocation_state_v1`.

Leggono lo stato astratto `post` prodotto dall'abstract interpretation. Un match
MAY vale `unk`; se il predicato è refutato vale `ff`.

### Predicati di evento

| Predicato | Evento |
|---|---|
| `alloc_l(v)` | allocazione |
| `drop_l(v)` | deallocazione/drop |
| `read_l(v)` | lettura |
| `write_l(v)` | scrittura |
| `use_l(v)` | uso, inclusi read/write dove previsto |

Con un `ProgramVar` sono label del nodo, sollevate attraverso l'alias information
esportata da CREMA. Con un `AbstractAllocId` schema v2 l'associazione
evento→allocazione è MAY: un witness positivo vale `unk`, l'assenza vale `ff`.

## Allocator mismatch

```cqpl
allocator_mismatch_l(a)
```

Alias accettato:

```cqpl
dealloc_mismatch_l(a)
```

Nel profilo corrente richiede:

```cqpl
requires allocation_contracts_v2;
```

`AllocatorMismatch-UB` controlla soltanto il mismatch della **famiglia**
allocator/deallocator. Non è una query generica per ogni forma di UB.

### `allocation_contracts_v2`

Famiglie correnti:

```text
rust_global
c_malloc
unknown
```

Ogni allocazione espone un `allocator_contract`; ogni allocation-label `drop`
espone un `deallocator_contract` proof-carrying. I campi v2 includono:

```text
family
operation
language
basis
owner_def_path       # audit-only, quando applicabile
allocator_def_path   # audit-only, quando applicabile
callee_def_path      # audit-only, quando applicabile
```

Le proof basis ammesse in v6N-r1a sono chiuse:

```text
rust_box_global_drop
rust_vec_global_drop
rust_global_dealloc_api
structural_c_free_v1
unresolved
```

`Box<_, Global>` e `Vec<_, Global>` sono classificati dal producer usando
identità rustc strutturali. `std::alloc::dealloc`/`alloc::alloc::dealloc` viene
certificato producer-side mentre il `DefId` è disponibile; il path serializzato
serve solo per audit e non viene reinterpretato dal checker.

Generic MIR `Drop`, `String`, `CString`, `Rc`, `Arc`, custom `Drop`, custom
allocator e call riconosciute solo da pretty text non vengono promosse a
`rust_global` in v6N-r1a: restano `unknown` salvo futura evidence strutturale.

La regola family-level è:

```text
rust_global -> rust_global : nessun mismatch witness
c_malloc    -> c_malloc    : nessun mismatch witness
rust_global -> c_malloc    : possibile mismatch
c_malloc    -> rust_global : possibile mismatch
unknown     -> ...         : non refutabile
...         -> unknown     : non refutabile
```

Poiché `AbstractAllocId` ed eventi di allocazione sono MAY, un witness positivo
di mismatch vale `unk`, non `tt`. Capability mancante o proof metadata v2
malformato produce un errore, mai una falsa refutazione `ff`.

## Query state-centric Leak / Double-Free / Use-After-Free

Il profilo corrente usa `allocation_state_v1`:

```text
leak_alloc_state.cqpl
double_free_alloc_state.cqpl
use_after_free_alloc_state.cqpl
```

`allocation_state_v1` è MAY-only. `allocation_post` è una proiezione pointwise
dello stato astratto esistente attraverso `AbstractAllocId`; non è una nuova
MUST lifecycle analysis.

## Operatori booleani e temporali

```text
!phi, not phi
phi && psi
phi || psi
EX phi, AX phi
EF phi, AF phi
EG phi, AG phi
E[phi U psi]
A[phi U psi]
```

Sono accettate anche forme come `E(F phi)`, `A(G phi)` e `E(X phi)`.

`X` è strong Next: su un terminale `EX phi` e `AX phi` valgono `ff`.

Il model checker usa fixed point sul reticolo:

```text
ff < unk < tt
```

## Sintassi riassuntiva

```text
phi ::= alloc(x) | drop(x) | own_forg(x)
      | alloc_l(v) | drop_l(v) | read_l(v) | write_l(v) | use_l(v)
      | allocator_mismatch_l(a)
      | !phi | phi && phi | phi || phi
      | exists x. phi | forall x. phi
      | exists_alloc a. phi | forall_alloc a. phi
      | EX phi | AX phi | EF phi | AF phi | EG phi | AG phi
      | E[phi U phi] | A[phi U phi]
```

Dove `x : ProgramVar`, `a : AbstractAllocId` e `v` dipende dal binding.

## Pipeline CREMA + CQPL

```text
1. CREMA analizza il Cargo project.
2. Costruisce l'ICFG Rust/C.
3. L'abstract interpretation produce pre/post.
4. Schema v2 aggiunge AbstractAllocId, allocation_post e allocation-event labels.
5. CREMA produce allocator/deallocator contracts e provenance supportata.
6. CREMA esporta annotated_icfg_v2.json.
7. CQPL valida schema e capability richieste dalla query.
8. Il model checker valuta la formula dall'entry.
9. Il risultato è ff, unk oppure tt.
```

CQPL non deduce allocator contract dai nomi dei nodi, dai pretty strings o dai
DefPath di audit: il producer CREMA deve classificare la famiglia.

## Esempio allocator mismatch v6N-r1a

Target:

```text
tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_box_from_raw_ub
```

Impostare i path:

```bash
ROOT=/home/af/Documenti/a-phd
TARGET="$ROOT/tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_box_from_raw_ub"
OUT=/tmp/cqpl-v6n-example
NIGHTLY=nightly-2024-11-21
mkdir -p "$OUT"
```

Compilare:

```bash
cargo +"$NIGHTLY" build --manifest-path "$TARGET/Cargo.toml"
```

Generare l'annotated ICFG schema v2:

```bash
rm -f "$ROOT/crema/global_icfg.json" "$ROOT/crema/ffi_functions.json"

(
  cd "$ROOT/crema"
  cargo +"$NIGHTLY" run -- \
    "$TARGET" \
    --entry main \
    --only-icfg-annotated \
    --cqpl-schema-version 2 \
    --annotated-icfg-out "$OUT/annotated_icfg_v2.json" \
    --allocation-identity-out "$OUT/allocation_identity.json"
)
```

Eseguire la query **v2**:

```bash
cargo +"$NIGHTLY" run \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" \
  -- \
  "$OUT/annotated_icfg_v2.json" \
  "$ROOT/cqpl/queries_v2/allocator_mismatch_ub_v2.cqpl"
```

Per `c_malloc -> Box<_, Global>` il risultato atteso resta:

```text
CQPL result: unk
```

La precisione v6N consiste nel fatto che il deallocator contract è ora
`rust_global` con basis `rust_box_global_drop`, non `unknown`.

Output JSON:

```bash
cargo +"$NIGHTLY" run \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" \
  -- \
  "$OUT/annotated_icfg_v2.json" \
  "$ROOT/cqpl/queries_v2/allocator_mismatch_ub_v2.cqpl" \
  --json
```

## Query personalizzata

Esempio `/tmp/my_query.cqpl`:

```cqpl
requires allocation_state_v1;

exists_alloc a. EF (alloc(a) && EX EF drop_l(a))
```

Esecuzione:

```bash
cargo +nightly-2024-11-21 run \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" \
  -- \
  "$OUT/annotated_icfg_v2.json" \
  /tmp/my_query.cqpl \
  --json
```

## CLI

```text
cqpl_checker <annotated-icfg.json> <query.cqpl>
  [--entry NODE_OR_FUNCTION]
  [--intra]
  [--bind x=PROGRAM_VAR_ID]...
  [--bind-alloc a=ABSTRACT_ALLOC_ID]...
  [--json]
```

- `--entry`: cambia entry/proiezione;
- `--intra`: limita alla funzione selezionata;
- `--bind`: lega una variabile libera a un `ProgramVar`;
- `--bind-alloc`: lega una variabile libera a un `AbstractAllocId`;
- `--json`: output machine-readable.

Le query ufficiali sono normalmente chiuse e non richiedono binding manuali.

## Annotated ICFG schema v2

Schema:

```text
schemas/annotated_icfg_v2.schema.json
```

Campi principali:

```text
schema_version, entry, capabilities, variables, allocations, nodes
nodes: successors, labels, allocation_labels, identity, event_identity, pre, post
```

`pre/post` provengono dall'abstract interpretation. `identity/event_identity`,
`allocation_post` e `allocation_labels` sono l'estensione allocation-centric MAY.
I contract allocator/deallocator sono metadata strutturali del producer.

## Corpus `run_all`

Il runner corrente deve eseguire schema v2 con:

```text
--contract-capability v2
```

e quindi usare:

```text
allocator_mismatch_ub_v2.cqpl
```

Il census resta intenzionalmente quello frozen a v6L (`110` discovered, `109`
active, con `no_errors_projects/openapi-client-gen` escluso). Il nome storico
`EXPECTED_TARGETS_V6L.txt` può quindi restare: descrive la provenienza del
snapshot, non la versione semantica delle query.

## Test

Regression suite corrente:

```bash
cargo +nightly-2024-11-21 test \
  --manifest-path cqpl/cqpl_checker/Cargo.toml
```

Nel gate v6N-r1a validato: CQPL lib `48/48`, CLI `7/7`, no-refutation `4/4`.
L'end-to-end allocator mismatch v2 viene validato dal corpus/focused harness,
non da una smoke query legacy.

## Boundary scientifico

Distinguere sempre:

1. core CQPL variable-centric;
2. schema v2 e `AbstractAllocId` MAY;
3. `allocation_state_v1` MAY-state projection;
4. soundness dell'abstract interpretation CREMA;
5. semantica three-valued del model checker;
6. `allocation_contracts_v1` come allocator-origin baseline;
7. `allocation_contracts_v2` come refinement proof-carrying dei deallocator contract.

In particolare, v6N-r1a non introduce MUST allocation/deallocation analysis e
non classifica genericamente tutti i Rust `Drop`. Le famiglie precise sono
emesse solo quando il producer dispone della proof basis supportata.