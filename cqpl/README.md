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
da solo, una prova di una concreta esecuzione del programma.

## Query disponibili

`queries/` contiene il core variable-centric originale.

`queries_v2/` è l'interfaccia corrente per gli esperimenti end-to-end e usa
`AbstractAllocId` per correlare la stessa possibile allocazione tra Rust e C.
Le query ufficiali sono:

```text
queries_v2/leak_alloc.cqpl
queries_v2/double_free_alloc.cqpl
queries_v2/use_after_free_alloc.cqpl
queries_v2/allocator_mismatch_ub.cqpl
```

Per nuovi esperimenti usare normalmente `queries_v2/`.

## Struttura di una query

Un file `.cqpl` contiene zero o più dichiarazioni `requires ...;` seguite da
una formula. Sono supportati commenti `#` e `//`.

```cqpl
requires allocation_contracts_v1;

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

Leggono lo stato astratto `post` prodotto dall'abstract interpretation.
Un match MAY vale `unk`; se il predicato è refutato vale `ff`.

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

### Allocator mismatch

```cqpl
allocator_mismatch_l(a)
```

Alias accettato:

```cqpl
dealloc_mismatch_l(a)
```

Richiede un `AbstractAllocId` e la capability:

```cqpl
requires allocation_contracts_v1;
```

`AllocatorMismatch-UB` controlla il mismatch della **famiglia** allocator /
deallocator. Non è una query generica per ogni forma di UB.

## `allocation_contracts_v1`

La query non definisce il contract: dichiara che richiede un annotated ICFG che
esponga questa capability.

L'artefatto contiene, per esempio:

```json
"capabilities": ["allocation_contracts_v1"]
```

Ogni allocazione ha un `allocator_contract` e ogni allocation-label `drop` ha
un `deallocator_contract`:

```json
{"family":"c_malloc","operation":"malloc","language":"c"}
```

Famiglie correnti:

```text
rust_global
c_malloc
unknown
```

Regola corrente:

```text
rust_global -> rust_global : compatibile
c_malloc    -> c_malloc    : compatibile
rust_global -> c_malloc    : possibile mismatch
c_malloc    -> rust_global : possibile mismatch
unknown     -> ...         : non refutabile
...         -> unknown     : non refutabile
```

Poiché gli `AbstractAllocId` sono MAY, un witness positivo di mismatch vale
`unk`, non `tt`. Capability mancante o artifact malformato produce un errore,
non `ff`.

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

## Come funziona CREMA + CQPL

```text
1. CREMA analizza il Cargo project.
2. Costruisce l'ICFG Rust/C.
3. L'abstract interpretation produce pre/post.
4. Schema v2 aggiunge AbstractAllocId e allocation-event labels.
5. CREMA esporta annotated_icfg_v2.json.
6. CQPL carica JSON e query e verifica le capability.
7. Il model checker valuta la formula dall'entry.
8. Il risultato è ff, unk oppure tt.
```

CQPL non deduce allocator contract dai nomi dei nodi: i contract sono metadata
strutturali prodotti da CREMA.

## Esempio completo su `tests_and_target_repos`

Useremo:

```text
tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_box_from_raw_ub
```

Impostare i path:

```bash
ROOT=/home/af/Documenti/a-phd
TARGET="$ROOT/tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_box_from_raw_ub"
OUT=/tmp/cqpl-example
mkdir -p "$OUT"
```

Compilare il target:

```bash
cargo +nightly-2024-11-21 build --manifest-path "$TARGET/Cargo.toml"
```

Generare l'annotated ICFG schema v2:

```bash
rm -f "$ROOT/crema/global_icfg.json" "$ROOT/crema/ffi_functions.json"

(
  cd "$ROOT/crema"
  cargo +nightly-2024-11-21 run -- \
    "$TARGET" \
    --entry main \
    --only-icfg-annotated \
    --cqpl-schema-version 2 \
    --annotated-icfg-out "$OUT/annotated_icfg_v2.json" \
    --allocation-identity-out "$OUT/allocation_identity.json"
)
```

Eseguire la query allocator mismatch:

```bash
cargo +nightly-2024-11-21 run \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" \
  -- \
  "$OUT/annotated_icfg_v2.json" \
  "$ROOT/cqpl/queries_v2/allocator_mismatch_ub.cqpl"
```

Risultato atteso per questo target:

```text
CQPL result: unk
```

Output JSON:

```bash
cargo +nightly-2024-11-21 run \
  --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" \
  -- \
  "$OUT/annotated_icfg_v2.json" \
  "$ROOT/cqpl/queries_v2/allocator_mismatch_ub.cqpl" \
  --json
```

## Query personalizzata

Esempio `/tmp/my_query.cqpl`:

```cqpl
exists_alloc a. EF (alloc_l(a) && EX EF drop_l(a))
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

`pre/post` provengono dall'abstract interpretation. `identity/event_identity` e
`allocation_labels` sono l'estensione MAY allocation-centric dello schema v2.

## Test

```bash
cargo +nightly-2024-11-21 test --manifest-path cqpl/cqpl_checker/Cargo.toml
```

## Boundary scientifico

Distinguere sempre:

1. core CQPL variable-centric;
2. estensione schema v2 con `AbstractAllocId`;
3. soundness dell'abstract interpretation CREMA;
4. semantica del model checker;
5. capability come `allocation_contracts_v1`.

Le allocation-label schema v2 sono MAY-only; `allocator_mismatch_l` controlla
solo la famiglia allocator/deallocator; capability mancante produce errore e
non una falsa refutazione `ff`.
