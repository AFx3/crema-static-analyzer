# B1.2 — Materialization & Empirical Admission Policy

Status: normative.
Scope: `cqpl/benchmarks/rustsec_memory_safety_v1/**` only.
Out of scope: `crema/**`, `cqpl/queries_v2/**`, `cqpl/cqpl_checker/**`,
`cqpl/library_models/**` (frozen at `cqpl-v6t-r1`).

---

## 1. Scopo

B1.1 seleziona 14 advisory RustSec come *candidate* documentali.
B1.2 trasforma ciascun candidate in una **coppia sperimentale concreta**
`(vulnerable_revision, fixed_revision)` con un **reproducer condiviso**,
materializzata in modo immutabile e verificabile.

B1.2 **non misura CREMA**. Non produce recall, precision, né metriche
di analisi. Produce il *ground truth empirico* che B1.3 userà per
interrogare CREMA.

---

## 2. Criterio di ammissione (normativo)

Un caso è `admitted` **se e solo se** soddisfa *tutte* le seguenti
condizioni:

1. `exact_vulnerable_revision` nota: version + commit (o assenza
   documentata di commit) + `source_archive_sha256` + `source_tree_sha256`.
2. `exact_fixed_revision` nota con gli stessi campi.
3. Entrambi i source tree sono **materializzati** sotto `cases/<id>/`
   e il loro hash coincide con quello registrato in `case.json`.
4. `provenance_complete`: advisory ID, URL RustSec, origine upstream
   (issue / patch / crate release).
5. `build_contract` esplicito: toolchain, target, features, native deps.
6. Entrambe le varianti **buildano** con lo stesso build contract
   (`build_status = pass`).
7. `same_trigger`: esiste un singolo file reproducer applicato
   identicamente alle due varianti.
8. `vulnerable_reproduces = true`: la variante vulnerabile mostra
   l'evidenza prevista (double-free, UAF, leak, panic-in-drop, ecc.).
9. `fixed_reproduces = false`: la variante fixed non mostra la stessa
   evidenza con lo stesso trigger e lo stesso criterio di osservazione.

Manca una sola condizione ⇒ il caso **non è admitted**.

Stati ammessi per `materialization_status`:

| stato          | significato                                                    |
|----------------|----------------------------------------------------------------|
| `candidate`    | ereditato da B1.1; nessuna materializzazione ancora tentata    |
| `materialized` | sorgenti scaricate, hashes calcolati, `case.json` scritto      |
| `admitted`     | tutte le 9 condizioni sono verificate                          |
| `excluded`     | materializzazione o validazione fallita, con `reason`          |

Motivi di esclusione ammessi (`reason`, enum chiusa):

- `source_unavailable`
- `source_integrity_mismatch`
- `build_incompatible`
- `reproducer_not_reproducible`
- `fixed_pair_not_equivalent`
- `native_dependency_unavailable`
- `advisory_ambiguous`

Ogni `reason` va accompagnata da una nota libera in `case.json`
campo `reason_detail`.

---

## 3. Regola del reproducer unico

Il reproducer è **uno solo** e viene eseguito contro entrambe le
varianti. Non sono ammessi `reproducer_A.rs` e `reproducer_B.rs`.

same_reproducer
├── linked against vulnerable
└── linked against fixed


L'unica variabile significativa è la revisione della crate.

---

## 4. Identità immutabile del sorgente

Non è sufficiente:
vulnerable = git main~1

Sono richiesti, per ogni variante:
- repository = <url>
- commit = <sha1|sha256|null>
- crate_version = <semver>
- source_archive_sha256 = <sha256 del .crate / tarball>
- source_tree_sha256 = <sha256 ricorsivo deterministico dell'albero>

Se il commit non è disponibile (es. release crates.io senza tag),
va registrato `commit = null` **e** la provenance deve indicare
l'origine crates.io con versione esatta.

---

## 5. Cosa B1.2 NON fa

- non modifica lo schema di CREMA
- non aggiunge query in `queries_v2/`
- non tocca `library_models/`
- non produce metriche di analisi
- non promuove casi sulla base di "compila" — solo su
  riproducibilità differenziale

---

## 6. Aggregati: quando si aggiornano

`subjects.tsv`, `ground_truth.json`, `candidate_selection.tsv`
si aggiornano **solo** dopo che un caso è passato a `admitted`
e `verify_b1_2.py` restituisce PASS.

Ordine operativo:
- materialize → materialized
- validate → admitted | excluded
- verify_b1_2 → PASS
- update aggregates (subjects / ground_truth / candidate_selection)
- verify_benchmark_v1 → PASS

Nessuna scrittura anticipata.

---

## 7. Rappresentazione di `ground_truth.json`

`ground_truth.json` **non** è un dict piatto `{case_id: entry}`. La sua
forma, già in B1.1, è:

```json
{
  "benchmark_id": "rustsec_memory_safety_v1",
  "schema_version": "rustsec_ground_truth_v1",
  "status": "candidate_set",
  "cases": [ {case_object}, {case_object}, ... ]
}
```

Ogni case_object è descritto da case_v1.schema.json.

### 7.1 B1.1 — case_object astratto

```json
{
  "case_id": "rustsec_2026_0282_aligned_box_realloc_panic",
  "advisory": "RUSTSEC-2026-0282",
  "crate": "aligned_box",
  "bug_family": "panic_unwind_memory_safety",
  "affected_function": "aligned_box::AlignedBox::realloc_with_default",
  "ground_truth_source": "https://rustsec.org/advisories/RUSTSEC-2026-0282.html",
  "panic_dependent": true,
  "ffi_dependent": false,
  "status": "candidate",
  "vulnerable": { "version_or_commit": "<0.3.1" },
  "fixed":      { "version_or_commit": ">=0.3.1" }
}
```
### 7.2 B1.2 — case_object admitted
```json
{
  "case_id": "rustsec_2026_0282_aligned_box_realloc_panic",
  "advisory": "RUSTSEC-2026-0282",
  "crate": "aligned_box",
  "bug_family": "panic_unwind_memory_safety",
  "affected_function": "aligned_box::AlignedBox::realloc_with_default",
  "ground_truth_source": "https://rustsec.org/advisories/RUSTSEC-2026-0282.html",
  "panic_dependent": true,
  "ffi_dependent": false,
  "status": "admitted",
  "vulnerable": {
    "version_or_commit": "0.3.0",
    "commit": "abc123...",
    "source_origin": "crates.io",
    "source_archive_sha256": "...",
    "source_tree_sha256": "..."
  },
  "fixed": {
    "version_or_commit": "0.3.1",
    "commit": "def456...",
    "source_origin": "crates.io",
    "source_archive_sha256": "...",
    "source_tree_sha256": "..."
  }
}
```