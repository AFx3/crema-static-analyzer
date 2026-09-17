#!/usr/bin/env python3
# cqpl/benchmarks/rustsec_memory_safety_v1/materialize_b1_2.py
"""
B1.2 materializer.

Scarica una coppia (vulnerable, fixed) da crates.io, la estrae in
modo deterministico, calcola hash di archivio e di albero, copia il
reproducer condiviso e scrive <out>/case.json con status "materialized".

NON ammette: la promozione a "admitted" è compito di verify_b1_2.py
dopo che la validazione differenziale è passata.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
import tarfile
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
CASES_DIR = HERE / "cases"
SELECTION_TSV = HERE / "candidate_selection.tsv"
CRATES_IO_DL = "https://crates.io/api/v1/crates/{crate}/{version}/download"
USER_AGENT = "b1.2-materializer/1.0 (+https://example.invalid)"
CHUNK = 1 << 20


# ---------------------------------------------------------------------------
# Hashing deterministico
# ---------------------------------------------------------------------------

def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(CHUNK), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_tree(root: Path) -> str:
    h = hashlib.sha256()
    files = sorted(p for p in root.rglob("*") if p.is_file())
    for p in files:
        rel = p.relative_to(root).as_posix()
        h.update(rel.encode("utf-8"))
        h.update(b"\0")
        h.update(sha256_file(p).encode("ascii"))
        h.update(b"\n")
    return h.hexdigest()


# ---------------------------------------------------------------------------
# crates.io download + extract
# ---------------------------------------------------------------------------

def download_crate(crate: str, version: str, dst_dir: Path) -> tuple[Path, str]:
    dst_dir.mkdir(parents=True, exist_ok=True)
    archive = dst_dir / f"{crate}-{version}.crate"

    if archive.exists():
        return archive, sha256_file(archive)

    url = CRATES_IO_DL.format(crate=crate, version=version)
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = resp.read()
    except Exception as e:  # noqa: BLE001
        raise RuntimeError(
            f"download failed for {crate}-{version}: {e}"
        ) from e

    archive.write_bytes(data)
    return archive, sha256_bytes(data)


def extract_crate(archive: Path, dst_dir: Path) -> None:
    if dst_dir.exists():
        shutil.rmtree(dst_dir)
    dst_dir.mkdir(parents=True)

    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        tops = {m.name.split("/", 1)[0] for m in members if "/" in m.name}
        if len(tops) != 1:
            raise RuntimeError(
                f"unexpected crate layout in {archive.name}: {tops}"
            )
        prefix = next(iter(tops)) + "/"

        for m in members:
            if not m.name.startswith(prefix):
                continue
            rel = m.name[len(prefix):]
            if not rel:
                continue
            if m.isreg():
                src = tar.extractfile(m)
                if src is None:
                    continue
                data = src.read()
                out = dst_dir / rel
                out.parent.mkdir(parents=True, exist_ok=True)
                out.write_bytes(data)
            elif m.isdir():
                (dst_dir / rel).mkdir(parents=True, exist_ok=True)


# ---------------------------------------------------------------------------
# case_id / selection
# ---------------------------------------------------------------------------

_CASE_ID_RE = re.compile(r"[^a-z0-9]+")


def slugify(s: str) -> str:
    return _CASE_ID_RE.sub("_", s.lower()).strip("_")


def read_selection_row(case_id: str) -> dict:
    if not SELECTION_TSV.exists():
        raise RuntimeError(f"missing {SELECTION_TSV}")
    with SELECTION_TSV.open("r", encoding="utf-8") as f:
        header = f.readline().rstrip("\n").split("\t")
        for line in f:
            row = dict(zip(header, line.rstrip("\n").split("\t")))
            if row.get("case_id") == case_id:
                return row
    raise RuntimeError(
        f"case_id {case_id!r} not found in {SELECTION_TSV.name}"
    )


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def build_case_id(advisory: str, crate: str, label: str) -> str:
    m = re.match(r"RUSTSEC-(\d{4})-(\d{4})", advisory)
    if not m:
        raise ValueError(f"unexpected advisory id: {advisory}")
    year, num = m.group(1), m.group(2)
    return f"rustsec_{year}_{num}_{slugify(crate)}_{slugify(label)}"


def parse_args(argv: list[str]) -> argparse.Namespace:
    ap = argparse.ArgumentParser(description="B1.2 materializer")
    ap.add_argument("--advisory", help="RUSTSEC-YYYY-NNNN")
    ap.add_argument("--crate", help="crate name")
    ap.add_argument(
        "--from-selection",
        help="case_id from candidate_selection.tsv",
    )
    ap.add_argument(
        "--label",
        help="slug for case_id (e.g. realloc_panic)",
    )
    ap.add_argument("--vuln-version", required=True)
    ap.add_argument("--fixed-version", required=True)
    ap.add_argument("--vuln-commit", default=None)
    ap.add_argument("--fixed-commit", default=None)
    ap.add_argument(
        "--reproducer", required=True, help="path to reproducer file"
    )
    ap.add_argument(
        "--provenance",
        default="crates.io",
        help="provenance string (e.g. 'crates.io', 'upstream-issue#123')",
    )
    ap.add_argument(
        "--out",
        default=None,
        help="output case dir (default: cases/<case_id>)",
    )
    ap.add_argument(
        "--force",
        action="store_true",
        help="overwrite existing case dir",
    )
    return ap.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)

    advisory = args.advisory
    crate = args.crate
    label = args.label

    if args.from_selection:
        row = read_selection_row(args.from_selection)
        advisory = row.get("advisory") or advisory
        crate = row.get("crate") or crate
        label = label or row.get("case_id") or "case"

    if not advisory or not crate or not label:
        print(
            "error: --advisory/--crate/--label required "
            "(or use --from-selection and provide --label)",
            file=sys.stderr,
        )
        return 2

    case_id = build_case_id(advisory, crate, label)
    out_dir = Path(args.out) if args.out else (CASES_DIR / case_id)
    if out_dir.exists() and not args.force:
        print(
            f"error: {out_dir} already exists (use --force)",
            file=sys.stderr,
        )
        return 3
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)

    vuln_dir = out_dir / "vulnerable"
    fixed_dir = out_dir / "fixed"
    evidence = out_dir / "evidence"
    repro_dir = out_dir / "reproducer"
    for d in (vuln_dir, fixed_dir, evidence, repro_dir):
        d.mkdir(parents=True, exist_ok=True)

    vuln_archive, vuln_archive_sha = download_crate(
        crate, args.vuln_version, evidence
    )
    fixed_archive, fixed_archive_sha = download_crate(
        crate, args.fixed_version, evidence
    )

    extract_crate(vuln_archive, vuln_dir)
    extract_crate(fixed_archive, fixed_dir)

    vuln_tree_sha = sha256_tree(vuln_dir)
    fixed_tree_sha = sha256_tree(fixed_dir)

    src_repro = Path(args.reproducer).resolve()
    if not src_repro.is_file():
        print(
            f"error: reproducer not found: {src_repro}",
            file=sys.stderr,
        )
        return 4
    dst_repro = repro_dir / src_repro.name
    shutil.copy2(src_repro, dst_repro)
    repro_sha = sha256_file(dst_repro)

    record = {
        "schema": "b1.2-case-v1",
        "case_id": case_id,
        "advisory": advisory,
        "crate": crate,
        "materialization_status": "materialized",
        "reason_if_not_admitted": None,
        "reason_detail": None,
        "vulnerable": {
            "version": args.vuln_version,
            "commit": args.vuln_commit,
            "source_origin": args.provenance,
            "source_archive_sha256": vuln_archive_sha,
            "source_tree_sha256": vuln_tree_sha,
            "build_status": "pending",
        },
        "fixed": {
            "version": args.fixed_version,
            "commit": args.fixed_commit,
            "source_origin": args.provenance,
            "source_archive_sha256": fixed_archive_sha,
            "source_tree_sha256": fixed_tree_sha,
            "build_status": "pending",
        },
        "build_contract": {
            "rust_toolchain": None,
            "target": None,
            "cargo_features": [],
            "native_dependencies": [],
        },
        "reproducer": {
            "file": f"reproducer/{dst_repro.name}",
            "sha256": repro_sha,
            "provenance": args.provenance,
            "expected_vulnerable_outcome": None,
            "expected_fixed_outcome": None,
        },
        "validation": {
            "vulnerable_reproduces": None,
            "fixed_reproduces": None,
        },
    }
    (out_dir / "case.json").write_text(
        json.dumps(record, indent=2, sort_keys=False) + "\n",
        encoding="utf-8",
    )

    (evidence / "source_sha256.txt").write_text(
        f"vulnerable.archive={vuln_archive_sha}\n"
        f"vulnerable.tree={vuln_tree_sha}\n"
        f"fixed.archive={fixed_archive_sha}\n"
        f"fixed.tree={fixed_tree_sha}\n"
        f"reproducer.sha256={repro_sha}\n",
        encoding="utf-8",
    )

    (out_dir / "README.md").write_text(
        f"# {case_id}\n\n"
        f"- advisory: `{advisory}`\n"
        f"- crate: `{crate}`\n"
        f"- vulnerable: `{args.vuln_version}` "
        f"(tree sha256 `{vuln_tree_sha[:16]}…`)\n"
        f"- fixed: `{args.fixed_version}` "
        f"(tree sha256 `{fixed_tree_sha[:16]}…`)\n\n"
        f"## Stato\n\n"
        f"`materialized` — in attesa di validazione differenziale.\n\n"
        f"## Validazione richiesta\n\n"
        f"1. compilare `vulnerable/` e `fixed/` con lo stesso build "
        f"contract;\n"
        f"2. eseguire `{dst_repro.name}` contro entrambe le varianti;\n"
        f"3. popolare `evidence/build.json` e `evidence/reproduce.json`;\n"
        f"4. aggiornare `case.json`: `build_status`, `validation.*`, "
        f"`materialization_status`.\n",
        encoding="utf-8",
    )

    (evidence / "build.json").write_text(
        json.dumps({"status": "pending"}, indent=2) + "\n",
        encoding="utf-8",
    )
    (evidence / "reproduce.json").write_text(
        json.dumps({"status": "pending"}, indent=2) + "\n",
        encoding="utf-8",
    )

    print(f"materialized: {out_dir}")
    print(f"case_id      : {case_id}")
    print(f"vulnerable   : {args.vuln_version} tree={vuln_tree_sha}")
    print(f"fixed        : {args.fixed_version} tree={fixed_tree_sha}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))