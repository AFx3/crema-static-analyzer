# CQPL — CREMA Query/Property Language

CQPL è il checker temporale three-valued che interroga il modello astratto prodotto da CREMA.

Per il flusso corrente, partire da **[ANALYSIS_PIPELINE.md](ANALYSIS_PIPELINE.md)**. Quel documento distingue in modo compatto:

```text
evidence producer
    -> annotated_icfg_v2
    -> Kripke
    -> result ff|unk|tt
    -> explanation
    -> subresult / direction / strength
```

## Stato scientifico corrente

La baseline semantica frozen delle 12 query è:

```text
subjects = 112
queries  = 12
attempts = 1344

ff  = 705
unk = 413
tt  = 226
```

R2 aggiunge explainability/provenance senza modificare questa truth semantics.

Distribuzione UNKNOWN validata in R2-R1.1:

```text
unk_true       = 273
unk_unoriented = 140

strong_abstract_evidence = 73
observational_candidate  = 200
unresolved               = 140
```

`unk_true` **non significa `tt`**: significa che esiste evidence direzionale positiva ma non sufficientemente forte da superare la semantica three-valued corrente. `unk_unoriented` indica invece che l'evidence disponibile non giustifica un orientamento.

La micro-release R2-R1.2 normalizza soltanto la provenance:

- token CString canonici;
- `pta_basis` citato solo con membership points-to non vuota;
- basis storico `structural_c_free_v1` preservato, con eventuale LLVM16/TLI corroboration separata;
- nessun cambiamento alle 12 query, al truth lattice o a `subresult/strength`.

## Pipeline in una frase

CREMA produce fatti astratti e proof-carrying evidence da MIR, Bmulti, LLVM16/TLI e SVF; CQPL valida l'artifact fail-closed, valuta la formula frozen sul Kripke e solo dopo costruisce explanation e assessment.

Documenti principali:

- [ANALYSIS_PIPELINE.md](ANALYSIS_PIPELINE.md) — flusso completo, semplice e corrente;
- [ANALYSIS_GUIDE.md](ANALYSIS_GUIDE.md) — comandi operativi;
- [ANNOTATED_ICFG.md](ANNOTATED_ICFG.md) — boundary CREMA -> CQPL;
- [EXPLAINABILITY.md](EXPLAINABILITY.md) — contratto explanation/assessment;
- [QUERY_CATALOG.md](QUERY_CATALOG.md) — significato delle 12 query e conteggi frozen;
- `capabilities/*.md` — contratti normativi delle evidence capability.

## Regola metodologica

Il PASS FINAL112 significa coerenza e riproducibilità rispetto al protocollo dichiarato. Non è una claim di perfect accuracy o concrete-execution proof. Le query memory-safety restano conservative perché gli eventi e l'identity sono spesso MAY.

### v6S-r1: allocation disposition senza cambiare le query storiche

v6S-r1 aggiunge la capability artifact `allocation_disposition_v1`. CREMA serializza osservazioni MAY sull'evoluzione della responsabilità di cleanup di un `AbstractAllocId`: `Box::into_raw`, `Box::from_raw`, `Box::leak`, `mem::forget(Box)`, return escape, deallocation MAY e soprattutto `mem::drop(raw_pointer)` come **no-op sul pointee**.

B1.1 mantiene quel vocabolario v1 congelato e dichiara `allocation_disposition_v2` come refinement fail-closed per `CString::into_raw` / `CString::from_raw`. v2 richiede v1; entrambi restano MAY-only e non cambiano la truth semantics CQPL.

La Rust Reference specifica che copiare o droppare un raw pointer non influenza il lifecycle di altri valori; quindi una call `std::mem::drop(raw: *mut T)` non produce `drop_l(a)`, non produce `may_deallocate(a)` e non deve portare il pointee a `FREED`. La classificazione v6S è fatta dal producer usando rustc `DefId` e il tipo dell'argomento, non ricostruita nel checker da stringhe.

Tutti i record r1 hanno `certainty=may_abstract`. **Le 12 query esistenti non consumano questi record**, quindi l'accettazione v6S-r1 richiede una nuova run 112×12 con zero mismatch contro v6R. Per la spiegazione semplice e i tre esempi `boxed_bool__ml`, `clean_into_from_raw` e `drop_raw_ptr_no_free`, vedi [V6S_R1_GUIDE.md](V6S_R1_GUIDE.md). Il contratto normativo è [capabilities/allocation_disposition_v1.md](capabilities/allocation_disposition_v1.md).

## Come CREMA e CQPL si dividono il lavoro

CREMA produce il significato astratto; CQPL lo interroga.

CREMA calcola:

1. un ICFG globale Rust/C;
2. lo stato astratto `CellValue` per le variabili;
3. aliasing e identity MAY verso `AbstractAllocId`;
4. eventi `alloc/drop/read/write/use`;
5. contratti allocator/deallocator;
6. label strutturali MIR `stmt:*`, `rvalue:*`, `term:*`;
7. provenance v6S-r1 `allocation_disposition_v1` per eventi ownership/lifecycle certificati;
8. opzionalmente telemetry di semantic coverage per il pipeline library v6O/v6Q.

CQPL usa soltanto ciò che è serializzato nell'annotated ICFG. In particolare **il TaintState interno di CREMA non è un dominio di predicati CQPL**: serve a CREMA come componente ausiliaria MAY/provenance, ma non viene esportato come `taint_src`/`taint_snk`. Vedi [LANGUAGE.md](LANGUAGE.md) e [ANNOTATED_ICFG.md](ANNOTATED_ICFG.md).

## Il dominio astratto CREMA in breve

La memoria astratta usa `CellValue`:

```text
                         TOP
             /       /    |    \       \
            MB     IMMB   MV   FREED   BOXTIMES
             \       |    /
                    ALLOC
                      |
                    BOTTOM
```

Interpretazione implementativa:

- `BOTTOM`: nessuno stato concreto normalmente rappresentato;
- `BOXTIMES`: valore scalare/non-heap;
- `ALLOC`: cella heap allocata/owned;
- `FREED`: cella nota come liberata;
- `MB`: mutable borrow;
- `IMMB`: immutable borrow;
- `MV`: ownership dimenticata/raw;
- `TOP`: informazione completamente imprecisa.

L'ordine rilevante è `BOTTOM <= tutto`, `ALLOC <= MB|IMMB|MV`, `tutto <= TOP`; gli altri elementi non correlati sono incomparabili.

CQPL non interpreta questi valori in modo booleano: i predicati MAY positivi producono `unk`, non `tt`.

## Le tre classi di predicati CQPL

### 1. Stato astratto MAY

```cqpl
alloc(x)
drop(x)
own_forg(x)
repeat_drop(a)
```

Con `x : ProgramVar` leggono `post`; con `x : AbstractAllocId` leggono `allocation_post` e richiedono `allocation_state_v1`. `repeat_drop(a)` è allocation-only e, nella semantica sperimentale A3.7, richiede `panic_lifecycle_state_v2`: un witness MAY dà `unk`, l'assenza con coverage `complete` dà `ff`, mentre coverage `unresolved` dà `unk`. `tt` non è disponibile in v2.

Un match MAY positivo vale `unk`; l'esclusione vale `ff`. Questo vale anche per `repeat_drop`: il witness lifecycle `may_abstract` è diagnostico e viene spiegato dall'explainer, non promosso a `tt`.

Esempio:

```cqpl
exists_alloc a. EF (alloc(a) && EX EG !drop(a))
```

non significa “esiste sicuramente un leak”. Se il modello MAY non può escludere il pattern, il risultato è normalmente `unk`.

### 2. Eventi

```cqpl
alloc_l(v)
drop_l(v)
read_l(v)
write_l(v)
use_l(v)
allocator_mismatch_l(a)
```

Su `ProgramVar`, le label evento sono sintattiche ed esatte dopo il lift sulla componente alias locale: match=`tt`, assenza=`ff`.

Su `AbstractAllocId`, le `allocation_labels` schema-v2 sono `may_abstract`: match=`unk`, assenza=`ff`. `allocator_mismatch_l(a)` è quindi una **possibile incompatibilità di famiglia**, non una prova concreta di UB.

### 3. Presenza strutturale MIR

```cqpl
stmt_l(assign)
rvalue_l(checked_binary_op)
term_l(return)
```

Richiedono `mir_semantic_labels_v1`. Sono presenza esatta block-level: `tt` se la label è nel nodo, `ff` altrimenti. Non introducono da sole una transfer semantics e non rappresentano ordine fra statement nello stesso basic block.

## `ff`, `unk`, `tt`: interpretazione corretta

Il dominio CQPL è:

```text
ff < unk < tt
```

- `ff`: la formula è refutata nel modello astratto;
- `unk`: il modello MAY non consente né di provarla né di refutarla;
- `tt`: la formula è stabilita nel modello serializzato.

Per le query memory/allocator della release corrente, un witness allocation-centric è MAY. Quindi un esito positivo è normalmente `unk`, **non** `tt`.

Per le query strutturali MIR `stmt_l/rvalue_l/term_l`, invece, `tt` significa semplicemente che la categoria è presente su almeno un nodo raggiungibile secondo la formula.

Un errore di capability o di parsing **non è mai `ff`**: il checker termina con errore.

## Quick start: run finale completo

Dal repository root:

```bash
set +e

ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21

CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
"$ROOT/cqpl/run_all.sh"

RC=$?
echo "run_all_rc=$RC"
```

Gate finale atteso:

```text
CQPL RUN_ALL v6Q-r1c: PASS subjects=112 queries=12 attempts=1344
run_all_rc=0
```

`run_all.sh` esegue nell'ordine:

1. test e build del checker;
2. corpus frozen 109 con MIR-v2;
3. tre crate registry pinned in modalità library;
4. census esatto dei 112 grafi;
5. tutte le 12 query su tutti i grafi;
6. cross-check delle quattro proprietà canoniche;
7. regression gates e checksum di tutti gli output.

Per riusare crate registry già preparate:

```bash
CQPL_CRATES_IO_OFFLINE=1 "$ROOT/cqpl/run_all.sh"
```

Per pulire i Cargo target del corpus prima della run:

```bash
CQPL_CLEAN_TARGETS=1 "$ROOT/cqpl/run_all.sh"
```

## Analizzare un singolo target vulnerabile

La via raccomandata è:

```bash
python3 cqpl/scripts/run_one_target_v6q_r1c.py \
  --root /home/af/Documenti/a-phd \
  --relative-path 'a-code_full_rust/a-double_free_full_rust_literals/boxed_bool'
```

Lo script:

- usa il nightly pinned;
- applica `TARGET_ANALYSIS_CONFIG.tsv` e `entry_overrides.json` quando necessari;
- compila prima il target;
- esegue CREMA dalla directory `crema/` per preservare la risoluzione SVF;
- abilita `--mir-semantics-v2`;
- produce `annotated_icfg_v2.json` e `allocation_identity.json`;
- valida le capability;
- esegue tutte le 12 query;
- scrive `query-results.tsv`, `summary.json` e `SHA256SUMS`.

Per un esempio Rust/C FFI:

```bash
python3 cqpl/scripts/run_one_target_v6q_r1c.py \
  --root /home/af/Documenti/a-phd \
  --relative-path 'a-code_c_ffi/branch_df_mem_leak_ffi'
```

Vedi [ANALYSIS_GUIDE.md](ANALYSIS_GUIDE.md) per procedura manuale, custom query e debug.

## Eseguire una singola query su un artifact esistente

```bash
cargo +nightly-2024-11-21 build \
  --manifest-path cqpl/cqpl_checker/Cargo.toml

cqpl/cqpl_checker/target/debug/cqpl_checker \
  /path/to/annotated_icfg_v2.json \
  cqpl/queries_v2/use_after_free_alloc_state.cqpl \
  --json
```

Output tipico:

```json
{
  "result": "unk",
  "entry": "rust::main::bb0",
  "scope": "interprocedural",
  "query_file": ".../use_after_free_alloc_state.cqpl"
}
```

## Le 12 query finali

| Query | Classe | Significato sintetico | Valori attesi oggi |
|---|---|---|---|
| `double_free_alloc` | memory | due `drop_l` sulla stessa allocation senza nuova allocazione | `ff/unk` |
| `double_free_alloc_state` | memory | come sopra, lifecycle iniziale da `allocation_post` | `ff/unk` |
| `leak_alloc` | memory | evento alloc seguito da cammino massimale senza drop | `ff/unk` |
| `leak_alloc_state` | memory | stato alloc seguito da cammino massimale senza stato freed | `ff/unk` |
| `use_after_free_alloc` | memory | drop seguito da `use_l` prima di una nuova allocazione | `ff/unk` |
| `use_after_free_alloc_state` | memory | come sopra, lifecycle iniziale da stato | `ff/unk` |
| `allocator_mismatch_ub` | contract v1 | possibile famiglia allocator/deallocator incompatibile | `ff/unk` |
| `allocator_mismatch_ub_v2` | contract v2 | stessa proprietà con deallocator proof v2 | `ff/unk` |
| `mir_statement_presence` | structural | esiste `stmt:assign` | `ff/tt` |
| `mir_rvalue_presence` | structural | esiste `rvalue:checked_binary_op` | `ff/tt` |
| `mir_terminator_presence` | structural | esiste `term:return` | `ff/tt` |
| `mir_structural_allocator_example` | mixed | `term:drop` combinato con possibile mismatch | `ff/unk` |

Per formula, capability, semantica di ogni esito e limiti: [QUERY_CATALOG.md](QUERY_CATALOG.md).

## Crate registry pinned

Il protocollo è in `artifact/CRATES_IO_TARGETS.json`:

| crate | versione | mode | target | API root | feature policy |
|---|---:|---|---|---|---|
| unicode-ident | 1.0.18 | library | lib | `unicode_ident::is_xid_start` | default |
| ryu | 1.0.20 | library | lib | `ryu::pretty::format32` | `small` |
| memchr | 2.7.4 | library | lib | `memchr::memchr::memchr` | no-default-features |

Non esiste fallback silenzioso lib/bin: cambiare target o API root significa cambiare soggetto sperimentale.

`memchr` è il regression subject higher-order: la run accettata deve produrre evidence strutturale `OptionMap` e non deve contenere `UNRESOLVED_HIGHER_ORDER`.

## Cosa significa “taint” in questa codebase

CREMA possiede un componente ausiliario `TaintState`:

```text
block -> variable -> set of markers
```

con marker come `assign`, `free`, `use` e provenance `alloc_family:c_malloc`. Il join è union MAY. Serve a propagare informazione/provenance che non deve essere persa quando il `CellValue` si allarga, per esempio a `TOP`.

Questo **non è il linguaggio CQPL corrente**. Il boundary `cqpl_export.rs` è read-only e non serializza il TaintState. Le vecchie idee `taint_src:` / `taint_snk:` appartengono al prototipo storico e non devono essere usate per interpretare le query v6Q-r1c.

## File principali

```text
README.md                         overview e quick start
LANGUAGE.md                       semantica e grammatica CQPL
QUERY_CATALOG.md                  spiegazione delle 12 query
ANALYSIS_GUIDE.md                 analisi pratica di target/crate
ANNOTATED_ICFG.md                 contratto CREMA -> CQPL
FINAL_VALIDATION.md               evidenza e gate finali
FINALIZATION.md                   commit/push/tag della release
capabilities/                     capability normative
artifact/FINAL112_AUDIT.md        audit scientifico
regression/                       test/oracle storici e strategia
scripts/run_one_target_v6q_r1c.py analisi singolo target
EXPLAINABILITY_GUIDE.md           tutorial explainability passo-passo
V6S_R1_GUIDE.md                    guida allocation disposition + raw-pointer drop
NEXT_STEPS_V6S.md                  roadmap empirica verso leak query più precisa
EXPLAINABILITY.md                 contratto diagnostico normativo
run_all.sh                        protocollo finale 109+3+12
```

## Boundary scientifico

La release segue queste regole:

- non inferire semantica da pretty text se CREMA può certificare struttura/identità;
- allocator/deallocator family: contract strutturali del producer;
- `Option` higher-order: evidence rustc typed/structural, non suffix DefPath;
- DefPath serializzati: provenance/audit, non classifier CQPL;
- costruzioni unsupported/ambigue: conservative abstraction o fail-closed, mai precisione inventata;
- `unk` è un risultato semantico valido, non un errore;
- `PASS` del protocollo non è una misura di precisione o di security accuracy.

La semantica teorica resta volutamente piccola. Il supporto implementativo può coprire più MIR soltanto tramite transfer esatta o soundly conservative rispetto a quel dominio.

---

## v6R-r1: explainability osservazionale validata

v6R-r1 risponde alla domanda: **“perché questa query è `unk`, e quale parte del modello lo rende inconclusivo?”**

Uso minimo:

```bash
cqpl_checker annotated_icfg_v2.json queries_v2/leak_alloc.cqpl \
  --json \
  --explain-json leak.explain.json \
  --explain-max-witnesses 8
```

Il normale stdout continua a contenere il risultato CQPL. Il file separato `leak.explain.json` contiene:

- `reason_frontier`: cause osservate sulla dependency trace;
- `binding`: binding delle variabili logiche;
- `relevant_nodes`: nodi usati dalla spiegazione;
- `derivation`: passi logici/CTL;
- `atomic_observations`: atomi finali che supportano la spiegazione;
- `diagnostics`: gate di completezza/non-tautologia.

Una causa come `MAY_ALLOCATION` non significa “vulnerabilità confermata”: significa che l'atomo allocation-centric rilevante è MAY e quindi non può diventare `tt` con la semantica congelata.

La validation v6R-r1 ha verificato 1344/1344 risultati ordinari identici a v6Q-r1c, senza `unk` non spiegati e senza `tt` privi di witness.

Uso interattivo raccomandato per una run v2:

```bash
python3 scripts/run_one_target_v6q_r1c.py \
  --root "$ROOT" \
  --relative-path 'a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool' \
  --explain-unk-verbose
```

Ogni `unk` genera sempre il relativo `.explain.json`; con `--explain-unk-verbose` lo stesso report validato viene anche mostrato nel terminale. Gli esempi empirici validati includono leak, UAF, double-free, allocator mismatch noto, contratto allocator irrisolto e UNKNOWN senza finding positivo.

Documenti principali:

- [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md): guida semplice, campi JSON, uso di `--explain-unk-verbose` ed esempi reali leak/UAF/double-free/allocator-mismatch;
- [EXPLAINABILITY.md](EXPLAINABILITY.md): contratto scientifico e tassonomia normativa;
- [V6R_VALIDATION.md](V6R_VALIDATION.md): protocollo e risultati di acceptance;
- `artifact/V6R_RUNTIME_VALIDATION.json`: summary machine-readable della validation.

## Documentazione v6S-r1

- [V6S_R1_GUIDE.md](V6S_R1_GUIDE.md): spiegazione semplice, esempi e comportamento atteso;
- [capabilities/allocation_disposition_v1.md](capabilities/allocation_disposition_v1.md): contratto machine-readable;
- [V6S_VALIDATION.md](V6S_VALIDATION.md): protocollo di acceptance;
- [NEXT_STEPS_V6S.md](NEXT_STEPS_V6S.md): next steps dopo il census dei 105 leak unknown.
