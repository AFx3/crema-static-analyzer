# Finalizzazione Git della release v6Q-r1c

Questa release deve essere committata senza includere output sperimentali/generati.

## 1. Cosa appartiene alla release

La release combina:

- CREMA v6Q-r1b semantic implementation;
- CQPL v6Q-r1c parser/harness/docs;
- query/schema/capability e audit metadata final112.

Non appartengono al commit:

- `repro-results/`;
- `target/`;
- `__pycache__/` e `*.pyc`;
- `global_icfg*.json/.dot` generati;
- `cqpl_annotated_icfg.json` generato;
- `crema/allocation_identity.json` generato;
- stale `SVF-example/output/*`;
- backup locali;
- `.main.dot`, `project_tree.txt` e altri dump.

## 2. Snapshot di sicurezza

Prima di installare/stagiare:

```bash
ROOT=/home/af/Documenti/a-phd
STAMP=$(date -u +%Y%m%dT%H%M%SZ)

git -C "$ROOT" diff > "$ROOT/repro-results/pre-v6q-r1c-$STAMP.patch"
git -C "$ROOT" status --short > "$ROOT/repro-results/pre-v6q-r1c-$STAMP.status.txt"
```

## 3. Installare il tree CQPL finale

Estrarre il pacchetto finale e copiare il solo tree `cqpl/` nel repository. Non usare `git add .`.

## 4. Staging selettivo

Il pacchetto contiene `artifact/RELEASE_STAGE_PATHS.txt` con i path sorgente ammessi.

Da repository root:

```bash
while IFS= read -r p; do
  [[ -z "$p" || "$p" == \#* ]] && continue
  git add -- "$p"
done < cqpl/artifact/RELEASE_STAGE_PATHS.txt
```

Poi:

```bash
git status --short
git diff --cached --stat
git diff --cached --check
```

Nessun path `repro-results`, `target`, `SVF-example/output`, backup o generated ICFG deve comparire nello staging.

## 5. Gate pre-commit

```bash
python3 cqpl/scripts/verify_v6q_r1c_static.py cqpl
cargo +nightly-2024-11-21 test --manifest-path cqpl/cqpl_checker/Cargo.toml
```

Se vuoi un'ultima conferma end-to-end prima del tag, eseguire `cqpl/run_all.sh` con output in `repro-results/` e **non** stageare quell'output.

## 6. Commit

Messaggio suggerito:

```text
cqpl: freeze v6Q-r1c final112 protocol
```

Comando:

```bash
git commit -m 'cqpl: freeze v6Q-r1c final112 protocol'
```

## 7. Verifica post-commit

```bash
git status --short
git show --stat --oneline HEAD
git diff HEAD^ HEAD --check
```

Il working tree può ancora contenere output non tracciati di ricerca; ciò non invalida il commit, purché non siano staged/committed.

## 8. Push branch

```bash
git push origin cqpl1
```

## 9. Tag annotato

Solo dopo push e verifica remota:

```bash
git tag -a cqpl-v6Q-r1c-final112 -m 'CREMA/CQPL v6Q-r1c final112 freeze'
git push origin cqpl-v6Q-r1c-final112
```

Se il repository usa una convenzione di tag diversa, mantenere la convenzione del progetto invece di introdurne una nuova.

## 10. Freeze record

Conservare fuori dal commit o come release asset:

- `cqpl-v6q-r1c-final112.zip`;
- SHA-256 dell'archive;
- console log della confirmation run;
- graph-evidence bundle r1b;
- source release ZIP r1c.

Il commit contiene il protocollo e gli audit metadata; gli output pesanti restano evidenza riproducibile, non source tree.
