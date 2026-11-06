# CStringCargo source-variant finding audit

## Provenance and scope

This audit reads only the archived smoke at
`/mnt/wsx-storage/af/cqpl8-cstringcargo-source-variant-smoke-2026110`.
No query or analyzer was executed. The reviewed fixture is a source-level
variant of the original benchmark: the reachable `log::error!` call in the
`CString::new` error arm was replaced by `abort()`. The two consecutive
`CString::from_raw(c_name_ptr)` calls are unchanged.

The modified `src/main.rs` SHA-256 is
`583745a43c7d5c90e33413e0cd7814e53476bb43cfd56cad94b1032f4d118386`.
`Cargo.toml` and `Cargo.lock` remained unchanged (respectively
`2446eadd993fe06d04ea9a8daffa70bcf83c24424037bd660472a409cd5dda64`
and `4865a0e134e3472ada919e6c1b1b974b39c923bcde5f20ab8a5d28386cbf1271`).
The campaign provenance records policy `evidence-qualified`, the A3 capability
manifest hash `80ef6917c3524722ed4dc54b92556bf58d463e349f922106469fb6d0a94d0f52`,
and the target-specific capability
`a3_bodyless_enum_constructor_callback_v1`.

## Shared allocation identity

All nine primary supporting findings name exactly one abstract allocation:
`CString::new::<&str>` at main MIR `bb4`, context `[]`, grounded at
`src/main.rs:17`. Ownership is handed off by `CString::into_raw` at main
`bb8` (`src/main.rs:19`). Consequently, neither four DF records nor four UAF
records establish four concrete defects.

## DF_NORMAL

The four `repeated_drop_without_reallocation` findings are
`observational_candidate` records over the same allocation. They combine the
source drop nodes (`main::bb13` for line 29 and `main::bb15` for line 35) with
nested `CString`/`Vec`/`Box` drop-glue nodes. At source level these records
describe one defect: the CString reconstructed at line 29 is dropped, then the
same raw pointer is reconstructed and reclaimed again at line 35. The four
records are distinct abstract event pairs but duplicate representations under
source-defect identity. `unk_true` remains logical `unk`; the certificates use
MAY allocation/deallocation evidence and therefore do not prove the formula
true.

## UAF

Findings 0--2 all end at main `bb14`, grounded at the second
`CString::from_raw` on line 35. Their predecessor nodes are alternative layers
of the first CString destruction. They represent one source-level UAF: Rust's
`CString::from_raw` reconstructs the string by examining the C string, so its
second invocation accesses storage already reclaimed after line 29.

Finding 3 instead ends at main `bb12`, the *first* `from_raw` on line 29, and
uses a nested `Vec`/`Box` event as its preceding deallocation. This ordering is
not independently valid as a source-level UAF of the CString allocation and is
retained as an abstract candidate, not a second concrete error. Again,
`strong_abstract_evidence` plus `unk_true` is not a concrete proof because the
event/allocation identity remains MAY.

## ML

The sole finding is a `normal_return_open_manual_obligation` candidate for the
same allocation after `into_raw`. Its own summary states that no discharge is
*modeled* on the selected abstract witness and that an intervening call has
effects outside the disposition vocabulary. In the source, the first
`from_raw` reconstructs an owner whose immediate drop reclaims the allocation;
the second reconstruction then causes DF/UAF rather than a leak. The error arm
aborts before any successful CString allocation is handed off. Therefore this
finding is an operational FP against the frozen oracle (`ML=0`) and is also
source-refuted under the stated normal-completion leak semantics. It is not a
basis for changing the oracle. Behavior after the second invalid reconstruction
is already undefined, which further prevents treating the abstract normal
return as a concrete leak proof.

## DF_FULL comparison

`DF_FULL` reports the same allocation, four finding kinds, event pairs and
source anchors as `DF_NORMAL` in this smoke. It is retained only as a diagnostic
query and must not contribute a second DF cell or any RQ1 aggregate.

## Operational classification of the four primary cells

| Property | Oracle | Detection | Class | Qualification |
|---|---:|---:|---|---|
| ML | absent | yes | FP | One observational leak candidate, source-refuted. |
| DF_NORMAL | present | yes | TP | One real DF represented by four abstract findings. |
| UAF | present | yes | TP | One real second-`from_raw` UAF; one extra abstract candidate. |
| ALLOC_MISMATCH | absent | no | TN | `ff`, no supporting finding. |

These are crate-property classifications. Finding multiplicity does not alter
their count.

## Reporting pipeline status

`scripts/rq1_paper_pipeline.sh` only creates a fresh bootstrap, runs its FFI
guard and invokes `run_rq1_final.py`; it did not generate paper tables.
`scripts/report_rq1_tables.py` now performs reporting only. It requires one
coherent 700-row primary campaign, expands the verified 175-row count oracle,
checks unique primary keys and `DF_NORMAL`, excludes failed cells, computes
per-query and true micro-aggregate metrics, emits CSV/LaTeX plus a classified
TSV, and records hashes of inputs, binaries, queries, oracle, capabilities and
the normalized fixture source. It never reads `DF_FULL` for primary metrics.

The reporter has been prepared but not executed.
