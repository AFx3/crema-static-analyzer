# Roadmap scientifica dopo la Phase 5 di CREMA

Sì, ma metterei i passi in un ordine leggermente diverso da quello che proponi, perché così ogni fase valida direttamente una parte del modello teorico invece di accumulare euristiche implementative.

Il percorso scientificamente più pulito, a questo punto, è:

1. **freeze definitivo della Phase 5**;
2. **implementare CQPL + model checker**;
3. **implementare il meccanismo generale di summary per FFI non disponibile**;
4. **istanziare e validare quel meccanismo su una libc specifica/versionata**;
5. **solo dopo fare l'analisi sistematica di crate Rust/C reali**.

La cosa interessante è che il punto 3 non è una nuova idea da aggiungere alla teoria: **è già previsto dal tuo modello**.

## 1. Prima chiudi davvero Phase 5

Appena il frozen comparator restituisce:

```text
92 candidate
92 baseline
0 mismatches
exit 0
```

congela:

```text
CREMA Phase 5 / AI implementation
toolchain
source hashes
SVF binary hash
89/89 tests
16/16 C-origin
92/92 regression
```

Da quel momento eviterei di cambiare la semantica dell'AI mentre sviluppi CQPL. Se trovi un vero bug, nuova fase/versione; altrimenti hai una base sperimentale stabile.

### Dettaglio aggiuntivo: cosa significa "freeze" in senso scientifico

Per freeze intenderei qualcosa di riproducibile, non soltanto "non toccare più il codice". Conserva almeno:

- commit Git o snapshot sorgente;
- hash SHA-256 dei file principali modificati;
- hash del binario SVF usato;
- versione esatta di `rustc`, `cargo`, CMake e toolchain;
- corpus di test;
- baseline frozen-92;
- output normalizzati;
- log dei comparator;
- criteri di inclusione/esclusione del corpus;
- eventuali limiti noti della fase.

Questo ti permette di trattare la Phase 5 come una baseline sperimentale immutabile rispetto alla quale misurare CQPL e le fasi successive.

---

# 2. Il prossimo vero contributo dovrebbe essere CQPL

Questo è il passo più naturale perché nella tua architettura l'output del fixed point **non è il risultato finale dell'analizzatore**.

La tua descrizione dice già che l'ICFG annotato dal fixed point costituisce l'abstract Kripke sul quale CQPL viene model-checked.

Quindi sostituire:

```text
detect_memory_issues
```

con:

```text
ICFG + fixed point
        ↓
Abstract Kripke
        ↓
CQPL parser
        ↓
CQPL evaluator/model checker
        ↓
ff / unk / tt
        ↓
diagnostic
```

chiuderebbe la distanza più importante tra **paper e implementazione**.

Io lo farei in quattro sottopassi.

### 2.1 Parser + AST CQPL

Implementa prima solo la sintassi già formalizzata:

```text
StateFormula
  MayPredicate
  LabelPredicate
  Not
  And
  Or
  ExistsVar
  ForallVar
  ExistsPath
  ForallPath

PathFormula
  X
  U
  F
  G
  State
```

senza ancora occuparsi delle performance.

### Dettaglio aggiuntivo: parser e rappresentazione interna

Conviene separare chiaramente tre livelli:

```text
testo CQPL
   ↓ parser
AST sintattico
   ↓ elaborazione
formula tipata / validata
   ↓ evaluation
risultato su K#
```

In questo modo puoi diagnosticare separatamente:

- errori sintattici;
- variabili logiche non legate;
- predicati sconosciuti;
- operatori non supportati;
- errori semantici;
- risultato del model checking.

È inoltre utile mantenere nell'AST le posizioni sorgente della query, così da produrre diagnostica leggibile.

### 2.2 Costruzione esplicita di \(K^\#\)

Non fare leggere al model checker direttamente strutture interne sparse di CREMA.

Costruisci un'interfaccia esplicita:

```text
K# =
  blocks
  edges
  labels
  pre_state
  post_state
```

cioè esattamente:

\[
K^\#=(B,R,L_B,\sigma^\#_{lfp},\Pi^\#_{pre},\Pi^\#_{post}).
\]

Questo boundary è molto utile anche per testing.

### Dettaglio aggiuntivo: rendi K# serializzabile

Sarebbe molto utile definire anche una rappresentazione serializzabile dell'abstract Kripke, ad esempio JSON.

Questo permette di:

- testare CQPL senza rieseguire rustc/SVF;
- avere fixture piccole e deterministiche;
- confrontare due versioni del model checker sulla stessa struttura;
- costruire test sintetici per operatori temporali;
- separare errori dell'AI da errori di CQPL.

In pratica potresti avere:

```text
CREMA frontend/AI
        ↓
abstract_kripke.json
        ↓
CQPL model checker
```

anche se nella modalità normale tutto resta in memoria.

### 2.3 Atomic predicates esattamente come nella teoria

Per esempio:

```text
alloc(x)
drop(x)
own_forg(x)
```

devono interrogare il lattice:

\[
A_p \sqsubseteq \Pi^\#_{post}(b)(v).
\]

Se sì:

```text
unk
```

altrimenti:

```text
ff
```

come hai già definito.

Mentre:

```text
alloc_l
drop_l
read_l
write_l
use_l
```

interrogano i label e restituiscono:

```text
tt / ff
```

mai `unk`.

Questo rende il model checker molto più rigoroso del detector attuale.

### Dettaglio aggiuntivo: non trasformare `TOP` in un warning diretto

Questo è importante per restare coerenti con il tuo modello.

Se:

```text
Pi#_post(b)(v) = TOP
```

allora, poiché più atomi astratti sono sotto `TOP`, i corrispondenti may-predicate possono risultare `unk`.

Non devi invece fare:

```text
if value == TOP:
    report_memory_error()
```

L'incertezza deve propagarsi attraverso la formula CQPL e diventare rilevante solo quando influenza una proprietà richiesta.

### 2.4 Poi gli operatori temporali

A quel punto implementi:

```text
EX / AX
EF / AF
EG / AG
EU / AU
```

sulla semantica a tre valori.

Qui farei anche un lavoro teorico parallelo molto importante: il tuo teorema attuale dà no-false-negatives per i **positive atomic may predicates**, ma il testo stesso specifica correttamente che questo non implica automaticamente la soundness di formule CQPL arbitrarie con negazione e CTL.

Quindi uno dei prossimi risultati teorici forti potrebbe essere:

> soundness / no-refutation theorem for a CQPL fragment.

Per esempio dimostrare che per le query ufficiali:

```text
Leak
DF
UAF
```

un concrete witness non può produrre `ff`.

Questo sarebbe un contributo molto più importante che aggiungere altri casi a `detect_memory_issues`.

### Dettaglio aggiuntivo: prima implementa il frammento realmente usato

Non è necessario partire da un CTL completo e ottimizzato.

Puoi iniziare dal frammento necessario alle query memory-safety del paper:

- quantificazione sulle variabili;
- predicati may;
- predicati di label;
- `EX`;
- `EF`;
- `EG`;
- `EU`;
- negazione tridimensionale;
- congiunzione/disgiunzione.

Poi estendere verso il linguaggio completo.

Questo riduce il rischio di sviluppare meccanismi non ancora usati sperimentalmente.

---

# 3. Subito dopo CQPL: FFI summaries

Qui la tua intuizione sulla libc è corretta.

Ma farei **prima il framework di summary generale**, e **poi libc come prima istanza**.

Perché è esattamente quello che il tuo modello teorico già prevede.

Nel testo hai:

> Foreign calls follow the same argument-passing discipline, but use the externally supplied summary \(\mathcal T\llbracket F\rrbracket^a_{\sigma_{in}}\).

e immediatamente dopo specifichi che nell'implementazione attuale questo non è necessario quando riesci a inlineare la funzione C nell'ICFG.

Quindi puoi rendere l'implementazione aderente alla teoria con due modalità:

```text
FFI call
   |
   +-- body available
   |      ↓
   |   inline C/SVF ICFG
   |
   +-- body unavailable
          ↓
      apply summary T[F]#
```

Questo sarebbe molto elegante.

### Dettaglio aggiuntivo: precedenza delle sorgenti di informazione

Definirei una policy deterministica:

```text
1. body C disponibile e analizzabile
   → usa body/inlining

2. body non disponibile ma summary versionata presente
   → usa summary

3. body e summary assenti
   → conservative unknown-FFI summary
```

Non mescolerei body e summary implicitamente, salvo un'esplicita modalità di validation.

Questo rende chiaro quale fonte ha prodotto ogni effetto.

---

# 4. Ma attenzione: non analizzerei la crate Rust `libc` per ottenere gli effetti

Qui c'è una distinzione importante.

La crate Rust:

```text
libc
```

ti dà soprattutto:

```text
signatures
types
constants
ABI declarations
extern symbols
```

Non contiene normalmente l'implementazione comportamentale di:

```text
malloc
free
memcpy
strlen
...
```

Quindi può essere utilissima per sapere:

```text
symbol name
argument types
return type
target availability
```

ma **non è da sola la sorgente giusta per derivare “alloca/free/read/write”**.

Io separerei:

```text
Rust libc crate
    ↓
ABI/signature database

actual libc implementation/specification
    ↓
effect summaries
```

E pinerei esplicitamente:

```text
target triple
libc implementation
libc version
Rust libc crate version
```

Per esempio scegliendo inizialmente **una sola libc C concreta** e un solo target.

Non cercherei subito di supportare:

```text
glibc
musl
Apple libc
BSD libc
Windows CRT
```

tutte insieme.

Una sola versione è metodologicamente molto più difendibile.

### Dettaglio aggiuntivo: scegli una combinazione target/libc precisa

Per esempio, una configurazione iniziale potrebbe essere:

```text
target: x86_64-unknown-linux-gnu
libc: glibc X.Y
Rust libc crate: versione Z
```

oppure una configurazione musl, se più facile da rendere riproducibile.

L'importante non è quale scegli, ma che sia esplicitamente congelata e documentata.

---

# 5. Non memorizzerei “variabili allocate/free/usate per funzione”

Formalizzerei invece gli effetti rispetto ai **parametri formali e al return value**.

Questo è fondamentale per renderli riutilizzabili a ogni call-site.

Per esempio, non:

```text
malloc:
    variable p is allocated
```

ma:

```text
malloc(size):
    RET may denote a fresh allocation
    allocation-family = C malloc
    RET may be null
```

`free`:

```text
free(ptr):
    ARG0 may be deallocated
    null is a no-op
    requires compatible allocation origin
```

`memcpy`:

```text
memcpy(dst, src, n):
    READ  src[0..n)
    WRITE dst[0..n)
    RET aliases dst
```

`strlen`:

```text
strlen(s):
    READ through ARG0
    RET = scalar
```

`strdup`:

```text
strdup(s):
    READ ARG0
    RET may be fresh C allocation
    RET may be null
```

`getenv`:

```text
getenv(name):
    READ ARG0
    RET borrowed/non-owning pointer
    no fresh allocation transferred to caller
```

`posix_memalign` è ancora più interessante:

```text
posix_memalign(out, align, size):
    WRITE through ARG0
    on success:
        *ARG0 = fresh allocation
    return scalar status
```

E `realloc` deve avere una summary disgiuntiva/conservativa:

```text
success:
    old allocation may be freed
    RET refers to allocation of requested size

failure:
    RET = null
    old allocation remains live
```

Questo si sposa perfettamente con il fatto che avevi già deciso di non fingere precisione su `realloc`.

### Dettaglio aggiuntivo: effetti su memoria raggiungibile

Col tempo sarà utile distinguere:

```text
read(ARG0)
write(ARG0)
read_through(ARG0)
write_through(ARG0)
escape(ARG0)
free(ARG0)
return_alias(ARG0)
return_fresh(...)
write_out_param(ARG0, ...)
```

Perché una funzione come `memcpy` non “usa la variabile” in senso generico: legge attraverso una regione e scrive attraverso un'altra.

Per la prima implementazione puoi mantenere una granularità più semplice, purché sia dichiarata.

---

# 6. Definirei una piccola FFI Effect Language

Questa potrebbe diventare una parte molto bella del sistema.

Per esempio qualcosa concettualmente del tipo:

```text
fn malloc(arg0):
    return may_fresh_alloc family=c_malloc
    return may_null

fn free(arg0):
    dealloc arg0 family=c_malloc
    null_ok

fn memcpy(arg0, arg1, arg2):
    read  arg1
    write arg0
    return_alias arg0

fn strlen(arg0):
    read arg0

fn strdup(arg0):
    read arg0
    return may_fresh_alloc family=c_malloc
    return may_null
```

Non serve necessariamente esporla agli utenti: può essere JSON/YAML/Rust structs.

Internamente potresti avere:

```rust
struct ForeignSummary {
    symbol: String,
    reads: Vec<PlaceEffect>,
    writes: Vec<PlaceEffect>,
    allocations: Vec<AllocationEffect>,
    frees: Vec<FreeEffect>,
    aliases: Vec<AliasEffect>,
    provenance: Vec<ProvenanceEffect>,
    return_effect: ReturnEffect,
}
```

Questa summary alimenta **due cose diverse**:

```text
abstract transfer
+
ICFG labels
```

Questo punto è essenziale per CQPL.

Se una `free()` libc non viene inlined:

```text
free(p)
```

la summary dovrebbe:

1. aggiornare la componente astratta;
2. produrre/annotare l'evento:

```text
drop_l(p)
```

Così il model checker non ha bisogno del corpo C.

### Dettaglio aggiuntivo: conserva la provenance della summary

Ogni effetto derivato da una summary dovrebbe sapere da dove proviene:

```text
source = inlined_body
source = libc_summary
source = unknown_foreign
```

Questo è utile sia per debugging sia per evaluation.

Potresti perfino far comparire questa provenance nei report sperimentali.

---

# 7. Per le funzioni sconosciute serve anche una conservative unknown summary

Se Rust chiama:

```rust
extern "C" {
    fn mystery(p: *mut T);
}
```

e non hai:

- body,
- summary,

non puoi semplicemente ignorare la call.

Per una modalità sound dovresti avere qualcosa come:

```text
UnknownForeignSummary
```

che conservativamente assume, secondo i tipi dei parametri pointer:

```text
may read through p
may write through mutable p
may retain/escape p
possibly modify reachable foreign state
```

Sul `may free` sarei molto esplicito nel modello: dipende dal contratto che vuoi assumere per funzioni C arbitrarie. Se vuoi una soundness molto forte rispetto a C arbitrario, una funzione straniera che riceve un raw pointer può anche chiamare `free`; questo rende la summary molto distruttiva.

È proprio per evitare che:

```text
unknown C call = TOP ovunque
```

renda CREMA inutilizzabile che il database di summaries standard diventa importante.

### Dettaglio aggiuntivo: modalità strict e practical

Potrebbe essere utile avere due modalità:

```text
strict:
    unknown FFI = summary massimamente conservativa

practical:
    unknown FFI = policy meno distruttiva, dichiarata esplicitamente
```

Ma solo se la distinzione viene documentata chiaramente.

Per il paper, userei la modalità coerente con il claim di soundness che vuoi fare.

---

# 8. Libc come primo benchmark delle summaries è un'ottima scelta

E qui farei qualcosa di scientificamente molto forte: **differential validation**.

Scegli una libc/versione.

Per un sottoinsieme di funzioni:

```text
malloc
calloc
free
memcpy
memmove
memset
strlen
strcpy/strncpy
strdup
realloc
...
```

fai due analisi.

### Modalità A — body available

```text
Rust
 ↓ FFI
C libc body
 ↓
SVF
 ↓
inline ICFG
 ↓
AI
```

### Modalità B — body hidden

```text
Rust
 ↓ FFI
summary database
 ↓
AI
```

E confronti gli outcome CQPL.

Questo è un esperimento eccellente:

\[
Result_{inline}(P)
\quad\text{vs}\quad
Result_{summary}(P).
\]

Non pretenderai necessariamente identità degli stati astratti, ma puoi verificare almeno che la summary non perda i comportamenti rilevanti:

```text
summary should be ≥ / no-more-precise-than
the relevant concrete/body abstraction
```

e soprattutto:

```text
no memory-error query found with inlining
should become falsely refuted by the summary
```

per le proprietà coperte.

Questo può diventare un esperimento del paper.

### Dettaglio aggiuntivo: costruisci microbenchmark per ogni summary

Prima dei crate reali, crea piccoli programmi per ciascuna funzione:

```text
malloc_leak
malloc_free
calloc_leak
free_twice
memcpy_valid
memcpy_after_free
strdup_free
realloc_success_like
realloc_failure_like
```

e verifica body-vs-summary.

Questo rende ogni summary falsificabile indipendentemente dal resto del sistema.

---

# 9. Solo dopo passerei a crate Rust/C reali

Se inizi adesso a raccogliere centinaia di crate, rischi di trovare contemporaneamente:

```text
CQPL bug
ICFG bug
missing summary
libc declaration
custom C function
unsupported build script
alias issue
macro/generated binding issue
```

e non saprai quale layer stai valutando.

Dopo CQPL + summaries, invece, puoi stratificare il corpus.

Io userei almeno tre categorie:

| Categoria | Obiettivo |
|---|---|
| C source disponibile | validare inlining/SVF |
| libc/system FFI senza body | validare summaries |
| external unknown FFI | valutare conservative fallback |

E poi una quarta:

```text
real-world mixed Rust/C crates
```

come evaluation finale.

### Dettaglio aggiuntivo: metriche da raccogliere

Per ogni crate reale registrerei almeno:

- build success/failure;
- numero di funzioni FFI;
- percentuale di FFI con body disponibile;
- percentuale risolta da summary;
- percentuale unknown;
- numero di query `ff/unk/tt`;
- warning per categoria;
- tempo totale;
- tempo SVF;
- tempo AI;
- tempo CQPL;
- dimensione ICFG;
- numero di locals/alias groups;
- eventuali timeout.

Questo rende la valutazione molto più informativa di un semplice conteggio di bug.

---

# 10. Una roadmap che secondo me è molto solida

La farei così:

```text
PHASE 5
AI C-origin correctness
✓ fixed point
✓ aliases implementation
✓ FFI body inlining
✓ allocation provenance
✓ free accounting
✓ regression
        |
        v
PHASE 6
CQPL
- parser
- AST
- Abstract Kripke builder
- atomic predicates
- 3-valued Boolean semantics
- CTL model checker
- Leak/DF/UAF queries
- compare CQPL vs legacy detector
- remove/deprecate detector
        |
        v
PHASE 7
External FFI summaries
- abstract summary interface
- synthetic summary nodes/events
- unknown foreign fallback
- alias-aware actual→formal effects
- return/out-param effects
        |
        v
PHASE 8
libc case study
- pin target + libc implementation/version
- summary database
- malloc/free/read/write/alias effects
- validate summary vs inlined libc body
        |
        v
PHASE 9
Real-world evaluation
- Rust/C crates
- available C source
- libc/system FFI
- missing external dependencies
- precision/performance/coverage
```

## Una cosa che terrei molto esplicita nella tesi

Il core teorico dice essenzialmente:

\[
Local\to CellValue
\]

e dimostra sound over-approximation.

L'implementazione può dire:

\[
D^\#_{impl}
=
CellValue
\times Alias
\times COrigin
\times Event
\times FFI\ Summary
\times \ldots
\]

senza problemi, purché presenti gli altri componenti come **implementation refinements** e non come se fossero già compresi dal teorema sul semplice `CellValue`.

La stessa cosa vale per CQPL: il teorema atomico che hai adesso è già ben circoscritto — un concrete witness per un may predicate non può essere astrattamente refutato. Io estenderei gradualmente quella dimostrazione alla classe di formule usata realmente dal tool, invece di fare un claim globale prima di averlo provato.

### Dettaglio aggiuntivo: separa i claim

Nel paper distinguerei esplicitamente:

```text
Claim A — soundness del core abstract interpretation
Claim B — precision refinements dell'implementazione
Claim C — correttezza/soundness del frammento CQPL
Claim D — correttezza conservativa delle FFI summaries
Claim E — risultati sperimentali sui crate reali
```

In questo modo ogni claim ha assunzioni, teoremi e test propri.

---

## Sintesi finale

La tua intuizione è giusta, ma l'ordine che sceglierei è:

\[
\boxed{
\text{CQPL}
\rightarrow
\text{generic FFI summaries}
\rightarrow
\text{versioned libc summaries}
\rightarrow
\text{real Rust/C crates}
}
\]

e **non**:

\[
\text{scan lots of crates}
\rightarrow
\text{invent special cases}
\rightarrow
\text{CQPL later}.
\]

E per la libc: usa pure la crate Rust `libc` come fonte di **binding/signature/ABI**, ma gli effetti `alloc/free/read/write/alias` devono provenire dal contratto o dall'implementazione della libc C che hai esplicitamente scelto. Il tuo formalismo ha già il posto giusto dove inserirli: la summary astratta \(\mathcal T\llbracket F\rrbracket^a\) prevista proprio quando il foreign body non è disponibile.

## Ordine operativo consigliato, molto concreto

Se vuoi trasformare questa roadmap direttamente in attività di sviluppo, procederei così:

1. chiudi e archivia Phase 5;
2. crea modulo `cqpl/ast.rs`;
3. crea parser CQPL con test unitari;
4. definisci struttura `AbstractKripke`;
5. esporta pre/post-state e labels dal fixed point;
6. implementa `TaintMayHold`;
7. implementa `LabelHold`;
8. implementa la logica tridimensionale;
9. implementa prima `EX`, `EF`, `EG`, `EU`;
10. codifica Leak/DF/UAF come query CQPL;
11. esegui legacy detector e CQPL in parallelo sul corpus frozen;
12. richiedi equivalenza degli outcome sul corpus di regressione;
13. depreca `detect_memory_issues`;
14. definisci `ForeignSummary`;
15. implementa resolver body/summary/unknown;
16. crea summary sintetiche per `malloc`, `calloc`, `free`;
17. aggiungi `memcpy`, `memmove`, `memset`, `strlen`, `strdup`;
18. affronta `realloc` separatamente;
19. valida body-vs-summary;
20. solo allora scala verso crate Rust/C reali.

Questa sequenza mantiene una proprietà metodologica importante: **ogni nuova fase può essere validata contro una baseline già congelata**, invece di modificare contemporaneamente analisi, query language, FFI model e corpus reale.
