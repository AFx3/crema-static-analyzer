# CQPL — CREMA Query/Property Language

CQPL è il checker temporale three-valued usato per interrogare il grafo annotato prodotto da CREMA.

La pipeline reale della release v6R-r1 (con semantica v6Q-r1c congelata) è:

```text
Cargo project / crate
        |
        v
      CREMA
  Rust MIR + LLVM/SVF + abstract interpretation
        |
        |  schema-v2 boundary
        v
annotated_icfg_v2.json
        |
        v
  cqpl_checker
        |\
        | \
        |  +--> explanation.json (v6R, opt-in)
        v
   ff | unk | tt
```

CQPL **non ricompila il programma e non ricostruisce MIR/LLVM**. Il checker riceve un artifact già costruito da CREMA, ne valida schema, capability e riferimenti, costruisce il Kripke finito e valuta la formula dalla `entry` dichiarata.

## Release corrente

```text
CREMA-CQPL-v6R-r1
semantic baseline: CREMA-CQPL-v6Q-r1c
status: explainability observational runtime-validated freeze candidate
primary toolchain: nightly-2024-11-21
rustc: 1.84.0-nightly (3fee0f12e 2024-11-20)
```

v6R-r1 mantiene **byte-identiche la semantica CREMA e le 12 query congelate di v6Q-r1c**. Aggiunge soltanto un secondo passaggio diagnostico opt-in che spiega `unk` e costruisce witness astratti per `tt`/`unk`. La validation runtime ha verificato identità dei risultati ordinari su tutte le 1344 celle final112.

### Evidenza runtime finale

Il run finale r1c contiene:

- 109/109 target corpus attivi, su 110 discovered e 1 skip esplicito;
- 3/3 crate registry: `unicode-ident 1.0.18`, `ryu 1.0.20`, `memchr 2.7.4`;
- 112 grafi `annotated_icfg_v2.json`;
- 12 query in `queries_v2/`;
- 1344 valutazioni CQPL, tutte con return code 0;
- `ff=650`, `unk=468`, `tt=226`;
- matrice r1c identica 1344/1344 alla matrice r1b già auditata sui byte dei grafi;
- 112/112 grafi validati strutturalmente;
- 6894/6894 record di identity sidecar verificati contro le annotazioni dei grafi;
- sulle tre crate registry: `rvalue_unmodeled=0`, `statement_unmodeled=0`, `terminator_unmodeled=0` e nessun `UNRESOLVED_HIGHER_ORDER`.

Archivio runtime finale verificato:

```text
cqpl-v6q-r1c-final112.zip
SHA-256: bd77b63d67523346b4e3c3cd9554f9787d7685e0f46265912137d55a98a8e902
```

Il PASS significa **riproducibilità e coerenza rispetto al protocollo dichiarato**. Non significa perfect accuracy. In particolare la leak analysis è ancora molto conservativa: 105/112 soggetti risultano `unk` nelle due query leak.

### Evidenza explainability v6R-r1

La validation v6R-r1 sullo stesso final112 ha prodotto:

- 112 soggetti × 12 query = 1344 spiegazioni;
- `baseline_result_mismatches=0`;
- `ff=650`, `unk=468`, `tt=226`, identici a v6Q-r1c;
- 0 `unk` senza reason frontier;
- 0 `unk` senza origine atomica specifica;
- 0 `tt` senza witness;
- 0 `tt` senza atomic witness endpoint;
- per `leak_alloc` e `leak_alloc_state`: 105/112 `unk`, tutti 105/105 con `MAY_ALLOCATION` nella frontier.

Il bundle runtime esterno `explainability.zip` ha SHA-256 `f41117d1d2fc1838cc1ee830071b07673811765884c9d96e4229ebbd187247d4`. Il repository conserva summary e protocollo, non i 1344 JSON runtime.

Per una spiegazione didattica campo-per-campo e un esempio completo su `boxed_bool__ml`, vedi [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md).

## Come CREMA e CQPL si dividono il lavoro

CREMA produce il significato astratto; CQPL lo interroga.

CREMA calcola:

1. un ICFG globale Rust/C;
2. lo stato astratto `CellValue` per le variabili;
3. aliasing e identity MAY verso `AbstractAllocId`;
4. eventi `alloc/drop/read/write/use`;
5. contratti allocator/deallocator;
6. label strutturali MIR `stmt:*`, `rvalue:*`, `term:*`;
7. opzionalmente telemetry di semantic coverage per il pipeline library v6O/v6Q.

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
```

Con `x : ProgramVar` leggono `post`; con `x : AbstractAllocId` leggono `allocation_post` e richiedono `allocation_state_v1`.

Un match MAY positivo vale `unk`; l'esclusione vale `ff`.

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

Documenti principali:

- [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md): guida semplice, campi JSON ed esempio passo-passo `boxed_bool__ml`;
- [EXPLAINABILITY.md](EXPLAINABILITY.md): contratto scientifico e tassonomia normativa;
- [V6R_VALIDATION.md](V6R_VALIDATION.md): protocollo e risultati di acceptance;
- `artifact/V6R_RUNTIME_VALIDATION.json`: summary machine-readable della validation.
