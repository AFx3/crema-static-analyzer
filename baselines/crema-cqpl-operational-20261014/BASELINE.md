# Baseline operativa CREMA/CQPL — 2026-10-14

Branch: `cqpl8-heap-transfer-semantics`.

## Stato verificato
- CREMA: interprocedural allocation identity, continuation notification repair.
- `crema/src/identity.rs` SHA-256: `39f3dbdf915c5ef449052bd9c91015b1eb38fc3886f0d0f62284f9cd91ac7956`
- CQPL: assessment storico, `cqpl/cqpl_checker/src/explain.rs` SHA-256: `8d802262532fe5b66441e1fbe92d5f4c061d8a2b7e46e8f6947894f0f98d13f1`
- Corpus attivo: **19/19 crate, 5 query ciascuna (95/95)**.
- Riferimenti frozen presenti: 19/19; confronto di JSON risultati ed explanation riportato PASS.
- `openapi-client-gen` escluso esplicitamente dal corpus operativo.
- Le quattro query CQPL v3 e il relativo README/SUITE, inizialmente untracked,
  sono inclusi esplicitamente dopo controllo SHA-256 contro lo snapshot
  CQPL del 2026-10-14; i tre backup `explain.rs.pre-*` sono esclusi.
- Toolchain di riferimento: `nightly-2024-11-21`.
- `SVF-example` e' directory sorella di `crema`.
- Verdetto: compatibilita' operativa sul corpus; **non** prova formale di soundness.

## Evidenze complete (esterne alla repository)
- Integrazione: `/mnt/wsx-storage/af/crema-identity-repo-apply-20261014T132221Z-1087897`
- Verifica corpus: `/mnt/wsx-storage/af/crema-repo-cqpl-verify-20261014T132349Z-1090811`
- Ripristino CQPL: `/mnt/wsx-storage/af/cqpl-restore-20261009-20261014T110807Z-1064374`
- SHA-256 summary: `51dd1e4d6700cb2d006b6c83f7e52490982d689bd7d54d7d8fe41303a759acd2`

Il commit congela sorgenti, contratti, query e questo riepilogo.
Non include build output, SVF output, ICFG generati, run dir o cancellazioni
preesistenti nella working tree. Per riproduzione completa sono necessari
i target esterni e i tool già documentati.
