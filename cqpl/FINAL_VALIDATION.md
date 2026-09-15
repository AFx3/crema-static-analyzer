# v6Q-r1c final validation

## Stato

```text
CREMA-CQPL-v6Q-r1c
status=final112-runtime-validated
semantic_baseline=CREMA-CQPL-v6Q-r1b
```

## Evidenza runtime finale

Archivio:

```text
cqpl-v6q-r1c-final112.zip
SHA-256 bd77b63d67523346b4e3c3cd9554f9787d7685e0f46265912137d55a98a8e902
```

Verifiche sull'archivio fornito:

- `SHA256SUMS.FINAL`: 4295/4295 entry OK;
- subjects=112;
- queries=12;
- attempts=1344;
- query `rc!=0`: 0;
- truth counts `ff=650`, `unk=468`, `tt=226`;
- canonical-four crosscheck PASS;
- event/state regression PASS;
- allocator v1/v2 regression PASS;
- matrix r1c identica 1344/1344 alla matrice r1b graph-audited.

## Gate sorgente prima del commit

Da repository root:

```bash
set +e
ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21

python3 "$ROOT/cqpl/scripts/verify_v6q_r1c_static.py" "$ROOT/cqpl"
RC_STATIC=$?

cargo +"$NIGHTLY" test --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml"
RC_TEST=$?

echo "static_rc=$RC_STATIC"
echo "checker_tests_rc=$RC_TEST"
```

Entrambi devono essere 0.

Per un full confirmation run prima del tag:

```bash
OUT="$ROOT/repro-results/cqpl-v6q-r1c-final112-confirmation"

CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
CQPL_RUN_ALL_OUT="$OUT" \
"$ROOT/cqpl/run_all.sh"
```

Marker finale:

```text
CQPL RUN_ALL v6Q-r1c: PASS subjects=112 queries=12 attempts=1344
```
