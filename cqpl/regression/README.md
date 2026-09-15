# Regression e protocolli storici CQPL

Per la release v6Q-r1c il protocollo normativo corrente è:

```bash
CREMA_PHD_ROOT=/home/af/Documenti/a-phd \
CREMA_RUST_TOOLCHAIN=nightly-2024-11-21 \
./cqpl/run_all.sh
```

Questo esegue corpus frozen 109 + tre crate registry + 12 query su 112 soggetti.

## Analisi singolo target corrente

```bash
python3 cqpl/scripts/run_one_target_v6q_r1c.py \
  --root /home/af/Documenti/a-phd \
  --relative-path 'a-code_c_to_rust_alloc/c_malloc_rust_free_then_use_uaf'
```

## Harness regression storico

`regression/scripts/run_target_repo_cqpl.py` è mantenuto per riprodurre scope storici (`frozen92`, `phase5-focus16`, `all`) e per review/oracle engineering. Non è il protocollo final112 normativo.

Esempio:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope frozen92
```

Debug storico di un target:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope all \
  --only c_malloc_rust_free_then_use_uaf
```

## Documenti

- `SEMANTIC_SCOPE.md`: cosa significano le property correnti;
- `TEST_STRATEGY.md`: livelli di verifica final112;
- `PERFORMANCE_NOTES.md`: note storiche di performance.

Gli oracle legacy sono riferimento differenziale, non ground truth automatica.
