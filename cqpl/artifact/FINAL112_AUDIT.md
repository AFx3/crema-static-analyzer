# Final112 scientific audit — CREMA v6Q-r1b / CQPL v6Q-r1c final

## Decision

La release **v6Q-r1c final112 è accettata come freeze runtime-validated**.

La semantica CREMA è la baseline v6Q-r1b; r1c corregge il lato CQPL/harness/documentazione senza modificare il dominio astratto CREMA né le 12 formule congelate.

## Evidenza congelata

- corpus: 110 discovered, 109 active, 109 complete, 1 skip esplicito;
- registry: `unicode-ident 1.0.18`, `ryu 1.0.20`, `memchr 2.7.4`;
- grafi: 112;
- query: 12;
- valutazioni: 1344;
- truth counts: `ff=650`, `unk=468`, `tt=226`;
- graph validation failures: 0;
- independent query-result mismatches: 0;
- sidecar identity records: 6894/6894 match;
- source/oracle unresolved inconsistencies: 0.

## Evidenza runtime r1c

Archivio:

```text
cqpl-v6q-r1c-final112.zip
SHA-256 bd77b63d67523346b4e3c3cd9554f9787d7685e0f46265912137d55a98a8e902
```

Il suo `SHA256SUMS.FINAL` verifica 4295/4295 file. La matrice contiene 112 soggetti × 12 query = 1344 celle, tutte con `rc=0`.

Il confronto cella-per-cella fra la matrice runtime r1c e la matrice r1b già auditata sui byte dei grafi produce:

```text
mismatches = 0 / 1344
```

Quindi il parser/harness r1c non ha alterato i risultati delle 12 query frozen.

## Audit dei grafi

L'audit indipendente sui 112 `annotated_icfg_v2.json` verifica:

- schema/capability;
- ID nodo unici;
- entry e archi validi;
- variables/allocations dichiarate;
- contract metadata;
- allocation state/labels;
- identity/event_identity;
- structural MIR labels.

Totali osservati:

```text
nodes       16512
edges       22710
variables    6094
allocations   148
```

20/112 grafi contengono nodi globali non raggiungibili dalla selected entry; 12125 nodi sono globalmente presenti ma entry-unreachable. Questo è coerente con l'architettura: il checker valuta dalla `entry`, non sull'intero grafo come insieme indiscriminato.

## Identity sidecars

Il test corretto non confronta “numero nodi del grafo” e “numero record sidecar”. Normalizza invece ogni record `by_node`/`event_by_node` e lo confronta col medesimo node ID serializzato nell'annotated ICFG.

Risultato:

```text
identity records checked       3447
event_identity records checked 3447
total                           6894
mismatches                         0
```

## Source/oracle consistency

Classificazione dei 112 soggetti:

- 106 `RESULT_SOURCE_CONSISTENT`;
- 2 `RESULT_SOURCE_CONSISTENT_EXPLAINED_LEGACY_DEVIATION`;
- 4 `RESULT_CONSISTENT_NO_REFERENCE_ORACLE`;
- 0 inconsistenze irrisolte.

Le due deviazioni storiche spiegate sono:

1. `df_rand_cargo_c_ffi`: il detector legacy usa una nozione UAF più ampia; la formula CQPL corrente richiede un evento `use/read/write` post-free, mentre `Box::from_raw` non viene automaticamente classificato come `use_l`;
2. `unsized_struct`: appartiene a `no_errors_projects`; la sequenza raw/reconstruction preserva ownership e `ML=ff` è più coerente con il sorgente rispetto al vecchio positivo leak.

## Drift producer/parser trovato e corretto

CREMA può emettere 16 categorie terminator:

```text
goto switch_int unwind_resume unwind_terminate return unreachable drop call
tail_call assert yield coroutine_drop false_edge false_unwind inline_asm unhandled
```

Il parser r1b non accettava l'intero set. L'evidenza finale contiene due `term:unwind_terminate`, uno dei quali raggiungibile dall'entry (`rusant`).

Le 12 query frozen non chiedevano `unwind_terminate`, quindi le 1344 truth values r1b non erano invalidate; era però falsa la claim più forte “ogni label prodotta è interrogabile”.

r1c chiude il drift e testa l'intero vocabolario.

## Precision boundary

`leak_alloc` e `leak_alloc_state` risultano `unk` su 105/112 soggetti.

Questo è un limite di precisione, non:

- una prova che 105 programmi abbiano leak;
- una failure di soundness dimostrata;
- una ragione per convertire `unk` in `tt` o `ff`.

## Conclusione

La release final112 stabilisce:

1. riproducibilità del protocollo dichiarato;
2. coerenza producer/schema/checker;
3. stabilità delle 12 query frozen;
4. consistenza graph-level e sidecar-level;
5. nessuna incoerenza source/oracle irrisolta;
6. runtime validation r1c sul toolchain pinned.

Non stabilisce perfect precision né una prova end-to-end di assenza/presenza concreta di ogni memory error.
