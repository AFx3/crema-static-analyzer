# CQPL — model checker tridimensionale per CREMA

Questa cartella sostituisce il precedente prototipo CQPL basato su `taint_src`,
`taint_snk`, wildcard e riconoscimento euristico del tipo di vulnerabilità.
Quella implementazione non corrispondeva più al modello teorico corrente.

La nuova architettura segue direttamente il formalismo CQPL:

```text
CREMA
  │
  │  annotated ICFG only
  ▼
K# = (B, R, L_B, Pi#_pre, Pi#_post, implementation alias information)
  │
  ▼
CQPL parser
  │
  ▼
three-valued CTL model checker
  │
  └── ff / unk / tt
```

Il checker **non esegue CREMA**, non legge MIR, LLVM o SVF e non contiene
classificatori hard-coded `MemoryLeak/DoubleFree/UAF`. CREMA deve esportare un
solo ICFG annotato conforme a `schemas/annotated_icfg.schema.json`.

## Corrispondenza con il modello teorico

Il core implementato è:

- dominio di verità `B = {ff < unk < tt}`;
- `not(unk) = unk`;
- `and = meet`, `or = join`;
- predicati semantici may: `alloc(x)`, `drop(x)`, `own_forg(x)`;
- predicati sintattici di label: `alloc_l(x)`, `drop_l(x)`, `read_l(x)`,
  `write_l(x)`, `use_l(x)`;
- quantificatori `exists x.` e `forall x.`;
- quantificatori di cammino `E`, `A`;
- `X`, `F`, `G`, `U`;
- `X` è **strong Next**;
- la valutazione globale parte dall'entry block.

Un may-predicate positivo restituisce `unk`, non `tt`. Se l'atomo astratto non
è incluso nell'abstract post-state restituisce `ff`. Questo replica
`TaintMayHold` del modello.

I label-predicate sono esatti rispetto a `L_B` e restituiscono soltanto `tt` o
`ff`.

## Estensione implementativa Rust + C

La teoria usa intenzionalmente:

```text
Env : Var_logic ⇀ Local
```

per mantenere semplice il modello e i teoremi. Il checker implementativo usa:

```text
Env_impl : Var_logic ⇀ ProgramVar
ProgramVar = RustVar ∪ CVar ∪ OtherVar
```

Il dominio dei quantificatori contiene quindi **sia variabili Rust sia variabili
C** esportate dall'ICFG annotato.

Gli identificatori devono essere globalmente univoci e call-site scoped quando
necessario, ad esempio:

```text
rust::main::Local(_1)
c::free_wrapper::%1@callsite0
```

Questo evita collisioni tra `%1` di funzioni o repliche FFI diverse.

### Alias

Il modello teorico non espone esplicitamente l'aliasing; l'implementazione di
CREMA invece lo gestisce. Per mantenere il checker indipendente da MIR/LLVM,
CREMA esporta l'aliasing dentro le annotazioni `pre/post` di ciascun nodo, come
componenti `{aliases, value}` dell'`AbstractMemory`.

CQPL le usa così:

- un semantic may-predicate legge il `CellValue` della componente di `x` in `post`;
- un label-predicate su `x` può essere soddisfatto da un evento su un alias di
  `x` disponibile nel `pre` o `post` dello stesso nodo, anche cross-language.

Per esempio un `drop_l(x)` con `x` bindata a un raw pointer Rust può essere
soddisfatto da una `free` etichettata sulla corrispondente variabile C.

Questa è un'estensione dell'implementazione rispetto al core teorico, non un
allargamento implicito del teorema corrente.

## Query

Una query contiene una sola formula di stato CQPL. `#` e `//` introducono
commenti.

Esempio leak:

```cqpl
exists x. EF (alloc(x) && EX EG !drop(x))
```

Double free:

```cqpl
exists x. EF (
  alloc(x) &&
  EX EF (
    drop_l(x) &&
    EX E[(!alloc_l(x)) U drop_l(x)]
  )
)
```

Use-after-free:

```cqpl
exists x. EF (
  alloc(x) &&
  EX EF (
    drop_l(x) &&
    EX E[(!alloc_l(x)) U use_l(x)]
  )
)
```

Le tre query sono in `queries/` e non sono speciali per il checker.

## Sintassi supportata

```text
phi ::= alloc(x)
      | drop(x)
      | own_forg(x)
      | alloc_l(x)
      | drop_l(x)
      | read_l(x)
      | write_l(x)
      | use_l(x)
      | !phi
      | phi && phi
      | phi || phi
      | exists x. phi
      | forall x. phi
      | EX phi | AX phi
      | EF phi | AF phi
      | EG phi | AG phi
      | E[phi U phi]
      | A[phi U phi]
```

Sono inoltre accettate forme esplicite vicine alla notazione teorica, ad
esempio:

```cqpl
E(F (alloc(x) && EX EG !drop(x)))
```

## Input: annotated ICFG

Il checker non deve conoscere le strutture interne di CREMA. L'unico contratto
è il JSON `AnnotatedIcfg` versione 1.

Esempio minimale:

```json
{
  "schema_version": 1,
  "entry": "rust::main::bb0",
  "variables": [
    {"id":"rust::main::Local(_1)","language":"rust"},
    {"id":"c::f::%1@callsite0","language":"c"}
  ],
  "nodes": [
    {
      "id": "rust::main::bb0",
      "successors": [],
      "labels": [],
      "pre": {"cells": []},
      "post": {
        "cells": [
          {"aliases":["rust::main::Local(_1)"],"value":"TOP"}
        ]
      }
    }
  ]
}
```

Valori ammessi di `CellValue`:

```text
BOTTOM BOXTIMES ALLOC FREED MB IMMB MV TOP
```

L'ordine è quello della Phase 5 di CREMA:

```text
BOTTOM <= ogni valore
ALLOC <= MB, IMMB, MV
ogni valore <= TOP
```

Gli altri elementi non collegati sono incomparabili.

Vedi `ANNOTATED_ICFG.md` e lo JSON Schema in `schemas/`.

## Semantica temporale e fixed point

Il checker non enumera esplicitamente i cammini. Sul grafo finito calcola gli
operatori CTL mediante fixed point sul reticolo finito `ff < unk < tt`:

```text
EF(phi) = lfp Z. phi OR EX Z
AF(phi) = lfp Z. phi OR AX Z
EU(p,q) = lfp Z. q OR (p AND EX Z)
AU(p,q) = lfp Z. q OR (p AND AX Z)

EG(phi) = gfp Z. phi AND E-next_G Z
AG(phi) = gfp Z. phi AND A-next_G Z
```

Per `X`, un nodo terminale vale `ff` sia sotto `E` sia sotto `A`, perché CQPL
usa strong Next.

Per `G`, invece, su un cammino massimale finito non esistono posizioni future:
la continuazione dopo un terminale è vacuamente `tt`, quindi `G phi` al nodo
terminale coincide con `phi` al nodo stesso.

## Variabili logiche libere

Le query ufficiali dovrebbero essere **chiuse**, coerentemente con il modello.
Per debugging il CLI ammette binding iniziali:

```bash
--bind x='c::free_wrapper::%1@callsite0'
```

Il target può essere una variabile Rust o C. Una formula con variabili libere
non bindate viene rifiutata invece di assegnare loro un significato implicito.

## Uso

Da `cqpl/cqpl_checker`:

```bash
cargo run -- \
  ../fixtures/cross_language_uaf.json \
  ../queries/use_after_free.cqpl
```

Output atteso per un pattern potenziale basato su may information:

```text
CQPL result: unk
Interpretation: potential match; the sound may abstraction does not refute the queried pattern.
```

Output JSON:

```bash
cargo run -- ../fixtures/cross_language_uaf.json ../queries/use_after_free.cqpl --json
```

## Interpretazione dei risultati

- `ff`: il modello astratto annotato refuta il pattern richiesto;
- `unk`: il pattern non è refutato dalla may-analysis; è il normale risultato
  di un potenziale memory error dipendente da informazione semantica may;
- `tt`: la formula viene stabilita dalla composizione tridimensionale, tipicamente
  quando la parte decisiva dipende da label esatte.

`TOP` **non è un warning di per sé**. Influenza i may-predicate, i quali possono
restituire `unk`; è la formula CQPL completa a determinare il risultato.

## Test inclusi nel crate

I test coprono almeno:

- algebra tridimensionale;
- parsing della formula Leak teorica;
- parsing di `E[phi U psi]`;
- quantificazione anche su variabili C;
- binding esplicito verso variabili C;
- `TOP -> unk` per `alloc/drop/own_forg`;
- strong Next sui terminali;
- `G` sui cammini massimali finiti;
- alias cross-language per i label.

Eseguire:

```bash
cargo test
```

## Cosa NON è ancora rivendicato

Il checker è costruito per corrispondere alla semantica CQPL descritta nel
modello, ma bisogna distinguere i claim:

1. la soundness dell'abstract interpretation è quella del core teorico;
2. il teorema corrente sui may-predicate atomici non implica automaticamente la
   soundness di ogni formula CTL con negazione;
3. alias C/Rust e identificatori cross-language sono refinement
   implementativi;
4. prima di un claim end-to-end CQPL va dimostrato il teorema appropriato sul
   frammento di formule effettivamente usato (almeno Leak/DF/UAF).

Questa separazione è intenzionale.

## Corpus regression

`regression/` contains the scientific replication harness for running the three
official CQPL memory-error formulas over CREMA's real `tests_and_target_repos/`
corpus. It intentionally does not clone CREMA's internal unit tests. See
`regression/TEST_STRATEGY.md` for the separation between CQPL semantics,
producer/consumer boundary tests, and end-to-end target replication.
