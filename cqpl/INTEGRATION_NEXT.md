# Prossimo passo di integrazione con CREMA

Il model checker è intenzionalmente separato. Per collegarlo a CREMA serve una
sola modifica lato analizzatore: un exporter deterministico dell'Annotated ICFG
v1.

L'exporter deve essere eseguito **dopo il fixed point e dopo la costruzione delle
informazioni di alias implementative**, ma prima del legacy
`detect_mem_issues`.

Output suggerito:

```text
crema/annotated_icfg.json
```

Dati da proiettare:

```text
GlobalICFGOrdered.ordered_nodes/edges -> nodes + successors
ICFG event extraction               -> labels
AbstractState                        -> pre
block transfer from fixed point      -> post
AbstractMemory allocation components  -> pre/post cells {aliases,value}
Rust + scoped LLVM/SVF names         -> variables
```

Dopo che l'exporter sarà stabile, il confronto scientifico consigliato è:

```text
legacy detect_mem_issues outcome
vs
CQPL Leak / DF / UAF outcome
```

sul corpus frozen, senza modificare contemporaneamente l'AI.
