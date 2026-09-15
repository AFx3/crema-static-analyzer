# Dopo il freeze v6Q-r1c final112

La baseline corrente è congelata. Prima di ulteriori estensioni, creare un nuovo identificatore/versione se cambia uno dei seguenti elementi:

- rustc/toolchain;
- census corpus;
- crate registry/versione/feature/API root;
- set delle 12 query;
- schema/capability;
- semantica astratta CREMA;
- producer evidence higher-order;
- vocabolario MIR normalizzato.

## Priorità scientifiche successive

1. Migliorare precisione leak: 105/112 `unk` è il limite empirico principale della freeze.
2. Migrare altre API higher-order soltanto tramite evidence typed/structural del producer.
3. Aggiungere transfer MIR precise quando giustificate; altrimenti mantenere conservative `TOP`/MAY o fail-closed.
4. Mantenere producer/parser vocabulary sincronizzati con test automatici.
5. Non introdurre fallback target/API root nei benchmark pinned.
6. Separare sempre claim di coverage, soundness boundary e precision/accuracy.
