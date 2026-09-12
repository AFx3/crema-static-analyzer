# Contratto CREMA → CQPL: Annotated ICFG v1

CQPL deve dipendere da CREMA soltanto attraverso questo artefatto.

## Informazioni obbligatorie

CREMA esporta:

1. `entry`: nodo iniziale;
2. insieme finito `variables` di tutte le variabili interrogabili;
3. `nodes` con relazione di controllo `successors`;
4. `labels = L_B` per eventi sintattici;
5. `pre` e `post` abstract memory annotations;
6. componenti MAY-alias dentro le abstract memory `pre/post`.

### ProgramVar cross-language

Il dominio implementativo dei quantificatori è:

```text
ProgramVar = RustVar ∪ CVar ∪ OtherVar
```

Una C variable deve essere scoped abbastanza da non collidere con una variabile
omonima di un'altra funzione/callsite. Gli ID numerici LLVM/SVF non devono
essere esportati come semplici `36` o `%1` se vengono replicati nell'ICFG.

## Pre/post state

CQPL semantic may-predicates interrogano **post**, coerentemente con
`Pi#_post` del modello teorico. `pre` viene comunque esportato perché fa parte
dell'Abstract Kripke ed è utile per query/estensioni future.

Una variabile assente da una mappa `pre/post` viene trattata come `BOTTOM`, non
come `TOP`.

## Labels

Eventi supportati:

```text
alloc drop read write use
```

`use_l(x)` nel checker è vero se è presente un label `use`, `read` oppure
`write` su `x` o su un suo alias.

La distinzione semantic/syntactic è intenzionale:

```text
alloc(x), drop(x), own_forg(x) -> Pi#_post -> ff/unk
alloc_l(x), drop_l(x), ...      -> L_B       -> ff/tt
```

## Alias

Ogni `pre`/`post` contiene le componenti dell'`AbstractMemory` nella forma:

```json
{"aliases":["rust::...", "c::..."], "value":"ALLOC"}
```

Una variabile non può apparire in due componenti diverse della stessa memoria;
il checker rifiuta tale input. L'aliasing resta quindi program-point-sensitive,
come nell'implementazione di CREMA, invece di essere collassato in una closure
globale. Questo evita di duplicare nel model checker il parsing MIR/LLVM o la
logica union-find della fase di analisi.

## Totalizzazione del grafo

Il checker **non aggiunge self-loop ai terminali**. Questo è necessario per
mantenere `X` strong come nel modello CQPL. La semantica di `G` tratta invece
correttamente i cammini massimali finiti senza inventare nuovi nodi o archi.
