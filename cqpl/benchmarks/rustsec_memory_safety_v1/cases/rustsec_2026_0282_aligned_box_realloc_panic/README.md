# rustsec_2026_0282_aligned_box_realloc_panic

- advisory: `RUSTSEC-2026-0282`
- crate: `aligned_box`
- vulnerable: `0.3.0` (tree sha256 `5ac5f91e3efd7bf4…`)
- fixed: `0.3.1` (tree sha256 `13da751acb0a8b8f…`)

## Stato

`materialized` — in attesa di validazione differenziale.

## Validazione richiesta

1. compilare `vulnerable/` e `fixed/` con lo stesso build contract;
2. eseguire `main.rs` contro entrambe le varianti;
3. popolare `evidence/build.json` e `evidence/reproduce.json`;
4. aggiornare `case.json`: `build_status`, `validation.*`, `materialization_status`.
