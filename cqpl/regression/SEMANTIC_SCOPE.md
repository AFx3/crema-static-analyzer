# CQPL regression semantic scope — v6Q-r1c final112

La release finale distingue fra **proprietà memory/allocator** e **probe strutturali MIR**.

## Proprietà memory/allocator

Le famiglie interrogate sono:

1. Leak;
2. Double Free;
3. Use After Free;
4. Allocator-family mismatch.

L'allocator mismatch è una classe specifica di UB legata all'incompatibilità di famiglia allocator/deallocator; non è una query generica per ogni UB Rust/C.

Per DF, Leak e UAF sono congelate due formulazioni:

- event-centric (`*_alloc.cqpl`);
- state-start (`*_alloc_state.cqpl`).

Per allocator mismatch sono congelate capability v1 e v2.

Queste coppie coincidono su 112/112 soggetti della freeze final112; la coincidenza è un regression fact, non un'identità semantica garantita per future versioni.

## Structural MIR probes

La matrice include:

```text
mir_statement_presence
mir_rvalue_presence
mir_terminator_presence
mir_structural_allocator_example
```

I primi tre verificano presenza strutturale e non sono vulnerability verdicts. Il quarto mostra composizione fra un structural fact esatto e un allocator MAY predicate.

## Perché non esiste un singolo `NO_ERRORS`

La negazione delle query correnti non equivale a una prova generale di memory safety:

- il set di proprietà è finito;
- `!unk = unk`;
- la sovra-approssimazione può rimanere imprecisa;
- unsupported future properties non sono coperte.

Quindi una serie di `ff` significa soltanto che quelle formule sono refutate nel modello corrente.

## Truth interpretation

Per le property allocation-centric correnti:

```text
ff  -> pattern refutato nell'astrazione
unk -> pattern possibile/non refutabile
tt  -> non atteso per witness MAY positivi nello schema corrente
```

Per structural MIR labels:

```text
ff -> categoria assente nel relativo scope raggiungibile
tt -> categoria presente
```

## Legacy detector

I risultati del detector storico CREMA sono reference differenziale, non ground truth CQPL.

Una divergenza richiede source/artifact review. La freeze final112 documenta due deviazioni legacy spiegate (`df_rand_cargo_c_ffi`, `unsized_struct`) e zero incoerenze irrisolte.
