# Migrazione dal vecchio prototipo CQPL

Il branch precedente conserva la storia del prototipo. La nuova cartella non
mantiene le seguenti idee perché non appartengono al modello teorico corrente:

- `taint_src:` / `taint_snk:`;
- `|>` come operatore temporale ad hoc;
- wildcard di statement;
- dichiarazioni `v: type|qualifier|name`;
- inferenza automatica `MemoryLeak/DoubleFree/UAF` dalla forma della regola;
- valutazione booleana di may-predicates;
- esecuzione di CREMA dal parser CQPL;
- metainterprete basato su equivalenza euristica delle regole.

Sono invece state conservate come idee utili:

- crate Rust separato;
- serializzazione JSON come boundary;
- parser testabile indipendentemente;
- quantificazione su un dominio finito.

Il vecchio `cqpl_soundness.v` non viene copiato nella nuova cartella perché
formalizza il precedente linguaggio con `FThen` e non la corrente semantica
CQPL/CTL tridimensionale. Rimane recuperabile dalla storia Git del branch
`cqpl` e potrà essere riscritto quando verrà formalizzato il teorema end-to-end
per il nuovo frammento.
