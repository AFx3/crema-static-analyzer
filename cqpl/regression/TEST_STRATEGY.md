# Strategia di test CQPL v6Q-r1c

La release separa esplicitamente cinque livelli di verifica.

## Layer 1 — unit semantics del checker

I test Rust coprono:

- parser e capability declarations;
- sort checking ProgramVar/AbstractAllocId;
- truth lattice `ff < unk < tt`;
- label ProgramVar e allocation-centric;
- allocator mismatch;
- strong `X`;
- fixed point di `F`, `G`, `U`;
- quantificatori;
- intero vocabolario MIR terminator prodotto da CREMA v6Q.

Non duplicano i test delle transfer CREMA.

## Layer 2 — boundary CREMA → CQPL

Il checker valida indipendentemente:

- schema e capability;
- closure del grafo;
- domini variables/allocations;
- alias components;
- allocation identity/state;
- contract allocator/deallocator;
- structural MIR labels.

Unsupported/malformed è hard error, non `ff`.

## Layer 3 — corpus frozen 109

`run_all.sh` usa `scripts/run_corpus_allocator_contracts.py` con:

```text
110 discovered
109 active
1 skipped: no_errors_projects/openapi-client-gen
```

Ogni target viene buildato col nightly pinned e analizzato con `--mir-semantics-v2` mantenendo target config ed entry override congelati.

Il corpus runner calcola anche le quattro proprietà canoniche Leak/DF/UAF/allocator-mismatch, poi la matrice finale le cross-checka.

## Layer 4 — tre crate registry

Le tre analisi sono explicit library subjects:

- unicode-ident 1.0.18;
- ryu 1.0.20;
- memchr 2.7.4.

Versione, feature e API root sono pinned. Nessun fallback silenzioso target-kind è ammesso.

Il gate richiede inoltre semantic coverage senza statement/rvalue/terminator unmodeled e, per memchr, higher-order Option evidence senza unresolved.

## Layer 5 — matrice final112

112 grafi × 12 query = 1344 celle.

Gate:

- 112 soggetti esatti;
- 12 query esatte;
- 1344 invocazioni;
- tutti `rc=0`;
- risultati solo `ff/unk/tt`;
- cross-check quattro query canoniche;
- coppie event/state uguali nella freeze;
- allocator v1/v2 uguali nella freeze;
- `mir_terminator_presence=tt` per 112/112 nella freeze;
- checksum di tutti gli output.

## Audit indipendente final112

L'evidenza è stata inoltre verificata sui byte dei 112 grafi:

- 112/112 graph validation PASS;
- 1344/1344 query-result replay match;
- 6894/6894 sidecar identity records match;
- 0 source/oracle inconsistency irrisolta.

Questo audit ha trovato il drift `term:unwind_terminate` producer/parser, corretto in r1c.

## Policy sugli oracle

Gli oracle storici/reviewed non vengono promossi automaticamente. Una differenza richiede review di:

- sorgente target;
- entry;
- grafo;
- state/labels/identity;
- query esatta.

`unk` non è failure e non è vulnerability proof.
