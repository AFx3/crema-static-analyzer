# CQPL v6R-r1 — linguaggio, semantica e interpretazione (baseline semantica v6Q-r1c)

Questo documento descrive il linguaggio **effettivamente implementato** dal checker v6Q-r1c. La sorgente normativa resta il codice in `cqpl_checker/src/`; questo file ne rende espliciti tipi, truth domain, capability e boundary.

## 1. Modello di esecuzione

CQPL valuta una formula su un Kripke finito costruito da `annotated_icfg_v2.json`:

```text
K = (Nodes, entry, successors, annotations)
```

Il risultato della query è il valore della formula nel nodo `entry`.

CQPL non esegue il programma concreto. Interroga una sovra-approssimazione prodotta da CREMA.

## 2. Truth domain three-valued

```text
ff < unk < tt
```

Operazioni logiche:

```text
!ff  = tt
!unk = unk
!tt  = ff

x && y = meet(x,y)
x || y = join(x,y)
```

Interpretazione:

- `ff`: il modello astratto refuta la formula;
- `unk`: il modello non basta per provarla o refutarla;
- `tt`: la formula è stabilita nel modello.

`unk` non è un parser error, timeout o failure del tool. Errori di schema/capability sono errori del processo e non vengono convertiti in `ff`.

## 3. Documento query e capability

Una query può iniziare con dichiarazioni:

```cqpl
requires allocation_state_v1;
requires mir_semantic_labels_v1;

exists_alloc a. EF (alloc(a) && EX EG !drop(a))
```

Le capability sono un contratto esplicito fra producer e query. Se la query richiede una capability non presente nell'artifact, il checker fallisce.

Capability final112 rilevanti:

```text
allocation_contracts_v1
allocation_contracts_v2
allocation_state_v1
mir_semantic_labels_v1
mir_semantics_v2
```

`mir_semantics_v2` è una capability di provenance del producer; non introduce un predicato CQPL diretto.

## 4. Sort logici

CQPL ha due domini quantificabili distinti:

```text
ProgramVar       -- variabile di programma Rust/C serializzata in variables[]
AbstractAllocId  -- identità astratta di allocazione serializzata in allocations[]
```

Quantificatori:

```cqpl
exists x. phi
forall x. phi
exists_alloc a. phi
forall_alloc a. phi
```

`exists/forall` bindano `ProgramVar`.

`exists_alloc/forall_alloc` bindano `AbstractAllocId` e richiedono schema v2.

Il sort checker viene eseguito prima del model checking. Per esempio `allocator_mismatch_l(x)` con `x : ProgramVar` è un errore di tipo.

## 5. Grammatica implementata

Forma semplificata:

```text
phi ::= alloc(x) | drop(x) | own_forg(x)
      | alloc_l(v) | drop_l(v) | read_l(v) | write_l(v) | use_l(v)
      | allocator_mismatch_l(a) | dealloc_mismatch_l(a)
      | stmt_l(statement_name)
      | rvalue_l(rvalue_family)
      | term_l(terminator_name)
      | !phi | not phi
      | phi && phi | phi || phi
      | exists x. phi | forall x. phi
      | exists_alloc a. phi | forall_alloc a. phi
      | EX phi | AX phi | EF phi | AF phi | EG phi | AG phi
      | E[phi U phi] | A[phi U phi]
      | E(X phi) | A(X phi) | E(F phi) | A(F phi) | E(G phi) | A(G phi)
```

Le forme `EX/AX/EF/AF/EG/AG` sono abbreviazioni delle forme path-quantified equivalenti implementate dal parser.

Precedenza, dalla più forte:

```text
! / not / quantificatori / operatori temporali unari
&&
||
```

Usare parentesi quando la portata non è ovvia.

## 6. Predicati di stato MAY: `alloc`, `drop`, `own_forg`

```cqpl
alloc(x)
drop(x)
own_forg(x)
```

### Su `ProgramVar`

Leggono `node.post` e confrontano il valore con il reticolo `CellValue`.

Mapping atomico:

```text
alloc    -> ALLOC
drop     -> FREED
own_forg -> MV
```

Il checker verifica `atom <= post(x)`.

Se sì: `unk`.

Se no: `ff`.

Non viene mai restituito `tt` per un predicato MAY positivo.

### Su `AbstractAllocId`

Richiedono:

```cqpl
requires allocation_state_v1;
```

Leggono `allocation_post`, che è una proiezione MAY dello stato `ProgramVar` attraverso l'identity analysis. La semantica three-valued è identica: match=`unk`, esclusione=`ff`.

### Effetto del reticolo

Dato l'ordine CREMA:

```text
ALLOC <= ALLOC, MB, IMMB, MV, TOP
FREED <= FREED, TOP
MV    <= MV, TOP
```

quindi per esempio `alloc(x)` è `unk` anche quando `post(x)=MV`: il valore astratto conserva la possibilità che la cella derivi da un'allocazione, non prova ownership concreta corrente.

## 7. Predicati evento su `ProgramVar`

```cqpl
alloc_l(x)
drop_l(x)
read_l(x)
write_l(x)
use_l(x)
```

Su `ProgramVar` sono label evento esatte del nodo, sollevate sulla componente alias locale ottenuta da `pre` e `post`.

Semantica:

```text
label presente su x o alias -> tt
label assente               -> ff
```

`use_l(x)` è vero per evento `use`, `read` o `write`.

Queste label sono diverse dai predicati MAY di stato. Una label evento indica che il producer ha annotato un evento in quel nodo; non afferma che l'intero stato heap sia noto con precisione MUST.

## 8. Predicati evento su `AbstractAllocId`

Gli stessi nomi possono essere applicati ad `a : AbstractAllocId`:

```cqpl
exists_alloc a. EF alloc_l(a)
```

Le `allocation_labels` schema-v2 sono ottenute dall'identity MAY e hanno:

```text
certainty = may_abstract
```

Quindi:

```text
match positivo -> unk
assenza        -> ff
```

Non esiste ancora una relazione MUST che permetta `tt` per questi atomi.

Questo è il motivo per cui le query memory final112 producono soltanto `ff/unk`.

## 9. Allocator/deallocator mismatch

```cqpl
allocator_mismatch_l(a)
dealloc_mismatch_l(a)
```

`dealloc_mismatch_l` è alias sintattico mantenuto per compatibilità.

Il predicato è valido soltanto su `AbstractAllocId` e richiede:

```cqpl
requires allocation_contracts_v1;
```

o:

```cqpl
requires allocation_contracts_v2;
```

Famiglie correnti:

```text
rust_global
c_malloc
unknown
```

A un evento `drop` il checker confronta `allocator_contract.family` con `deallocator_contract.family`.

È considerato possibile mismatch quando:

- le famiglie note sono diverse, oppure
- una delle famiglie è `unknown`.

Poiché la label allocation-centric è MAY, il risultato atomico positivo è `unk`.

Quindi `allocator_mismatch_l(a)=unk` significa “l'incompatibilità non è refutabile nel modello”, non “UB concretamente provata”.

## 10. Structural MIR labels

Richiedono:

```cqpl
requires mir_semantic_labels_v1;
```

Forme:

```cqpl
stmt_l(name)
rvalue_l(name)
term_l(name)
```

Sono formule chiuse: non bindano variabili logiche.

Semantica nodo-locale:

```text
semantic_labels contiene prefix:name -> tt
altrimenti                           -> ff
```

Non viene introdotta una semantica MAY. Sono label strutturali di presenza.

### Statement vocabulary

```text
assign
fake_read
set_discriminant
deinit
storage_live
storage_dead
retag
place_mention
ascribe_user_type
coverage
intrinsic
const_eval_counter
nop
backward_incompatible_drop_hint
other
```

### Rvalue vocabulary

```text
use
const
checked_binary_op
ptr_metadata
discriminant
len
nullary_op
copy_for_deref
address_of
ref
cast
binary_op
unary_op
repeat
thread_local_ref
shallow_init_box
aggregate
other
```

Le rvalue family sono un adapter versionato sul debug spelling del rustc pinned; un cambio toolchain richiede revalidazione.

### Terminator vocabulary v6Q-r1c

```text
goto
switch_int
unwind_resume
unwind_terminate
return
unreachable
drop
call
tail_call
assert
yield
coroutine_drop
false_edge
false_unwind
inline_asm
unhandled
```

r1c allinea il parser all'intero vocabolario emesso dal producer. L'audit final112 ha osservato realmente `term:unwind_terminate`.

Importante: “label interrogabile” non significa “transfer precisa”. Per esempio `tail_call` rimane un boundary conservativo/fail-closed dove la disciplina caller-pop non è modellata completamente.

## 11. Taint: componente CREMA, non predicato CQPL

CREMA mantiene anche:

```text
TaintState = block -> variable -> set<string>
```

Marker osservati nell'implementazione includono:

```text
assign
free
use
alloc_family:c_malloc
NOT_HANDLED
```

Il join del TaintState è una union MAY. Serve soprattutto a conservare provenance/informazione ausiliaria quando il `CellValue` diventa meno preciso.

**CQPL v6Q-r1c non espone predicati taint.** `cqpl_export.rs` dichiara esplicitamente che il boundary non muta né serializza il taint state. Le vecchie primitive `taint_src:` / `taint_snk:` non appartengono al linguaggio corrente.

Per parlare di “abstract predicate” in CQPL, i predicati corretti sono `alloc/drop/own_forg` e le allocation-centric labels MAY, non il TaintState interno.

## 12. Semantica temporale

### Next

```cqpl
EX phi
AX phi
```

`X` è **strong**.

A un nodo senza successori:

```text
EX phi = ff
AX phi = ff
```

CQPL non inventa self-loop terminali.

### Eventually

```cqpl
EF phi
AF phi
```

Sono least fixed points.

Intuizione:

- `EF phi`: esiste un cammino che raggiunge `phi`;
- `AF phi`: ogni cammino deve raggiungere `phi`.

### Globally

```cqpl
EG phi
AG phi
```

Sono greatest fixed points.

Su un cammino massimale finito, il nodo terminale controlla `phi` nella posizione corrente; la continuazione oltre il terminale è vacuamente `tt` per l'operatore G.

### Until

```cqpl
E[phi U psi]
A[phi U psi]
```

`psi` deve essere raggiunto; fino a quel punto `phi` deve valere secondo il path quantifier selezionato.

## 13. Quantificatori e aggregazione three-valued

Per un dominio finito:

```text
exists = join dei candidati
forall = meet dei candidati
```

Quindi:

- un candidato `tt` rende `exists` `tt`;
- se nessun candidato è `tt` ma almeno uno è `unk`, `exists` è `unk`;
- `exists` è `ff` solo se tutti i candidati sono `ff`.

Il checker implementa pruning conservativo di candidati per alcune formule esistenziali, ma soltanto quando può provare che i candidati rimossi contribuirebbero `ff`; il risultato semantico non cambia.

## 14. Query chiuse e binding manuali

Le query ufficiali sono chiuse.

Per debug il CLI può valutare formule aperte usando binding espliciti:

```text
--bind x=PROGRAM_VAR_ID
--bind-alloc a=ABSTRACT_ALLOC_ID
```

Un binding verso un ID inesistente è errore.

## 15. Errori hard vs truth values

Sono hard error, non `ff`:

- capability richiesta ma assente;
- schema non supportato;
- query con variabile logica non bindata;
- sort mismatch;
- entry/successor non valido;
- riferimento a variabile/allocation non dichiarata;
- metadata contract v2 invalido;
- structural label non normalizzata;
- sintassi/predicato non riconosciuto.

Questa distinzione è parte del boundary scientifico: “unsupported” non deve apparire come refutazione logica.

## 16. Esempi

### Presenza esatta di un return MIR

```cqpl
requires mir_semantic_labels_v1;
EF term_l(return)
```

- `tt`: esiste un nodo raggiungibile dall'entry con `term:return`;
- `ff`: nessun nodo raggiungibile soddisfa la label.

### Possibile leak allocation-centric

```cqpl
requires allocation_state_v1;
exists_alloc a. EF (
  alloc(a) &&
  EX EG !drop(a)
)
```

- `unk`: esiste un witness MAY che non può essere refutato;
- `ff`: il pattern è refutato nell'astrazione;
- `tt`: non è atteso con l'attuale semantica MAY degli atomi positivi.

### Possibile double free

```cqpl
exists_alloc a. EF (
  alloc_l(a) &&
  EX EF (
    drop_l(a) &&
    EX E[(!alloc_l(a)) U drop_l(a)]
  )
)
```

La seconda `drop_l(a)` deve apparire prima di una nuova `alloc_l(a)` lungo il suffix considerato.

### Combinare label strutturali e contratti

```cqpl
requires mir_semantic_labels_v1;
requires allocation_contracts_v2;
exists_alloc a. EF (
  term_l(drop) && allocator_mismatch_l(a)
)
```

`term_l(drop)` è esatto; `allocator_mismatch_l(a)` è MAY. La congiunzione può quindi risultare `unk` ma non diventa `tt` solo perché il terminatore è certo.

## 17. Le query ufficiali

La release final112 congela esattamente 12 file in `queries_v2/`. Vedi [QUERY_CATALOG.md](QUERY_CATALOG.md) per l'interpretazione individuale e i risultati della freeze.

## 18. Explainability v6R: estensione diagnostica, non linguistica

v6R non aggiunge costrutti alla grammatica CQPL e non aggiunge un quarto truth value. `ff < unk < tt`, CTL, quantificatori e predicati hanno esattamente la semantica v6Q-r1c.

L'explainer viene eseguito **dopo** la valutazione normale e deve restituire lo stesso `result`. Se risultato ordinario e risultato spiegato differiscono, il checker fallisce chiuso.

Il file `cqpl_explanation_v1` separa quattro livelli:

1. `result`: truth CQPL già calcolata;
2. `reason_frontier`: cause di incertezza osservate sulla dependency trace;
3. `derivation`: operatori logici/CTL attraversati;
4. `atomic_observations`: atomi terminali che supportano la spiegazione.

Esempio per un allocation-centric atom schema-v2:

```text
allocation label certainty = may_abstract
            |
            v
alloc_l(a) = unk
            |
            v
reason = MAY_ALLOCATION
```

`QUERY_THREE_VALUED_PROPAGATION` descrive soltanto la propagazione di quell'`unk` attraverso `&&`, quantificatori o CTL; da solo non è accettato come origine specifica dell'incertezza.

### Campi semantici principali

- `binding`: ambiente delle variabili logiche (`ProgramVar` o `AbstractAllocId`);
- `relevant_nodes`: nodi del Kripke necessari alla trace;
- `complete_dependency_trace`: completezza della trace selezionata, non prova di concrete execution;
- `atomic_observations[].truth`: truth dell'atomo con lo stesso dominio `ff|unk|tt`;
- `atomic_observations[].reasons`: cause diagnostiche direttamente supportate da grafo/derivazione.

Le reason `EXTERNAL_EFFECT`, `UNRESOLVED_ESCAPE`, `GLOBAL_TOP_EFFECT`, `CONTROL_FLOW_UNRESOLVED`, `HIGHER_ORDER_UNRESOLVED` restano riservate finché il producer non esporta provenance certificata. Non vengono dedotte da stringhe DefPath o dalla sola presenza di `TOP`.

Per la definizione campo-per-campo e un esempio eseguibile dal target `boxed_bool__ml`, vedi [EXPLAINABILITY_GUIDE.md](EXPLAINABILITY_GUIDE.md). Per il contratto normativo completo vedi [EXPLAINABILITY.md](EXPLAINABILITY.md).
