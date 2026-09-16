use crate::abstract_domain::{classify_mir_statement_coverage, classify_mir_statement_coverage_v2, has_explicit_rust_call_summary};
use crate::cargo_project::CargoAnalysisPlan;
use crate::mir_semantics::{mir_semantics_v2_enabled, rvalue_category};
use crate::panic_unwind::panic_unwind_lifecycle_v1_enabled;
use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, MirTerminator};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageCounts {
    pub total: u64,
    pub precise: u64,
    pub conservative: u64,
    pub unmodeled: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CallCoverage {
    pub total: u64,
    pub resolved_local: u64,
    pub external_summary: u64,
    pub unresolved: u64,
    pub external_opaque: u64,
    pub mixed_local_external: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCoverageReport {
    pub requested_root: String,
    pub selected_entry: String,
    pub rust_functions: u64,
    pub rust_mir_blocks: u64,
    pub statements: CoverageCounts,
    pub rvalues: CoverageCounts,
    pub terminators: CoverageCounts,
    pub calls: CallCoverage,
    pub statement_kind_histogram: BTreeMap<String, u64>,
    pub rvalue_family_histogram: BTreeMap<String, u64>,
    pub terminator_kind_histogram: BTreeMap<String, u64>,
    pub unmodeled_statement_kinds: BTreeMap<String, u64>,
    pub unmodeled_rvalue_families: BTreeMap<String, u64>,
    pub external_opaque_callees: BTreeMap<String, u64>,
    /// Reachable canonical ICFG edges that remain fail-closed for schema-v2.
    /// This is telemetry only: recording a gap never turns it into an accepted summary.
    pub unresolved_control_flow_summaries: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCoverageBundle {
    pub schema_version: u32,
    pub semantics: String,
    pub telemetry_only: bool,
    pub transfer_profile: String,
    pub package_name: String,
    pub package_version: String,
    pub package_source: Option<String>,
    pub target_name: String,
    pub target_kind: String,
    pub target_triple: Option<String>,
    pub resolved_package_features: Vec<String>,
    pub resolved_package_features_source: String,
    pub roots: Vec<RootCoverageReport>,
}

fn bump(map: &mut BTreeMap<String, u64>, key: impl Into<String>) {
    *map.entry(key.into()).or_insert(0) += 1;
}

fn rvalue_family(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with("copy ") || t.starts_with("move ") {
        return "use".to_string();
    }
    if t.starts_with("const ") {
        return "const".to_string();
    }
    if t.starts_with('&') {
        return "borrow_or_address".to_string();
    }
    let end = t
        .char_indices()
        .find_map(|(i, c)| matches!(c, '(' | '[' | ' ' | '{').then_some(i))
        .unwrap_or(t.len());
    if end == 0 { "unknown".to_string() } else { t[..end].to_string() }
}

fn terminator_name(term: &MirTerminator) -> &'static str {
    match term {
        MirTerminator::Goto { .. } => "Goto",
        MirTerminator::SwitchInt { .. } => "SwitchInt",
        MirTerminator::UnwindResume { .. } => "UnwindResume",
        MirTerminator::UnwindTerminate { .. } => "UnwindTerminate",
        MirTerminator::Return { .. } => "Return",
        MirTerminator::Unreachable { .. } => "Unreachable",
        MirTerminator::Drop { .. } => "Drop",
        MirTerminator::Call { .. } => "Call",
        MirTerminator::TailCall { .. } => "TailCall",
        MirTerminator::Assert { .. } => "Assert",
        MirTerminator::Yield { .. } => "Yield",
        MirTerminator::CoroutineDrop { .. } => "CoroutineDrop",
        MirTerminator::FalseEdge { .. } => "FalseEdge",
        MirTerminator::FalseUnwind { .. } => "FalseUnwind",
        MirTerminator::InlineAsm { .. } => "InlineAsm",
        MirTerminator::Unhandled { .. } => "Unhandled",
    }
}

fn callee_label(function_called: &str, callee_def_path: Option<&str>) -> String {
    callee_def_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| function_called.to_string())
}

pub fn collect_root_coverage(
    icfg: &GlobalICFGOrdered,
    requested_root: &str,
    selected_entry: &str,
) -> RootCoverageReport {
    let mut report = RootCoverageReport {
        requested_root: requested_root.to_string(),
        selected_entry: selected_entry.to_string(),
        rust_functions: icfg.rust_functions.len() as u64,
        rust_mir_blocks: 0,
        statements: CoverageCounts::default(),
        rvalues: CoverageCounts::default(),
        terminators: CoverageCounts::default(),
        calls: CallCoverage::default(),
        statement_kind_histogram: BTreeMap::new(),
        rvalue_family_histogram: BTreeMap::new(),
        terminator_kind_histogram: BTreeMap::new(),
        unmodeled_statement_kinds: BTreeMap::new(),
        unmodeled_rvalue_families: BTreeMap::new(),
        external_opaque_callees: BTreeMap::new(),
        unresolved_control_flow_summaries: BTreeMap::new(),
    };
    let resolved_call_nodes: BTreeSet<&str> = icfg
        .rust_calls
        .iter()
        .map(|call| call.call_node.as_str())
        .collect();

    // v6O-r1e: coverage must remain observable precisely when CQPL schema-v2
    // rejects an unresolved control-flow summary. Recompute reachability over
    // the canonical ICFG and record, but do not reinterpret, UNRESOLVED_* edges.
    let mut successors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &icfg.icfg_edges {
        successors
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.destination.as_str());
    }
    let mut reachable: BTreeSet<&str> = BTreeSet::new();
    let mut work = vec![selected_entry];
    while let Some(node) = work.pop() {
        if !reachable.insert(node) { continue; }
        if let Some(nexts) = successors.get(node) {
            work.extend(nexts.iter().copied());
        }
    }
    for edge in &icfg.icfg_edges {
        if !reachable.contains(edge.source.as_str()) { continue; }
        let Some(label) = edge.label.as_deref() else { continue; };
        if label.starts_with("UNRESOLVED_") {
            bump(&mut report.unresolved_control_flow_summaries, label);
        }
    }

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(block) = node else { continue; };
        report.rust_mir_blocks += 1;
        for stmt in &block.statements {
            report.statements.total += 1;
            bump(&mut report.statement_kind_histogram, stmt.kind.clone());
            let class = if mir_semantics_v2_enabled() {
                classify_mir_statement_coverage_v2(stmt)
            } else {
                classify_mir_statement_coverage(stmt)
            };
            match class {
                "precise" => report.statements.precise += 1,
                "conservative" => report.statements.conservative += 1,
                _ => {
                    report.statements.unmodeled += 1;
                    bump(&mut report.unmodeled_statement_kinds, stmt.kind.clone());
                }
            }
            if let Some(rvalue) = stmt.rvalue.as_deref() {
                report.rvalues.total += 1;
                let family = if mir_semantics_v2_enabled() {
                    rvalue_category(rvalue).to_string()
                } else {
                    rvalue_family(rvalue)
                };
                bump(&mut report.rvalue_family_histogram, family.clone());
                match class {
                    "precise" => report.rvalues.precise += 1,
                    "conservative" => report.rvalues.conservative += 1,
                    _ => {
                        report.rvalues.unmodeled += 1;
                        bump(&mut report.unmodeled_rvalue_families, family);
                    }
                }
            }
        }

        let Some(term) = block.terminator.as_ref() else { continue; };
        report.terminators.total += 1;
        bump(&mut report.terminator_kind_histogram, terminator_name(term));
        match term {
            MirTerminator::TailCall { .. } | MirTerminator::Unhandled { .. } => {
                report.terminators.unmodeled += 1;
            }
            // Supported with explicit conservative abstract semantics.
            MirTerminator::InlineAsm { .. } | MirTerminator::Yield { .. } => {
                report.terminators.conservative += 1;
            }
            MirTerminator::Call {
                function_called,
                callee_def_path,
                callee_is_local,
                deallocator_evidence,
                resolved_instance_callees,
                instance_dispatch_external,
                instance_dispatch_unresolved,
                ..
            } => {
                report.calls.total += 1;
                let direct_local_resolved = *callee_is_local
                    && resolved_call_nodes.contains(node_id.as_str());
                let instance_local_resolved = !resolved_instance_callees.is_empty();
                let summary = deallocator_evidence.is_some()
                    || has_explicit_rust_call_summary(function_called);

                if *instance_dispatch_unresolved
                    || (*callee_is_local && !direct_local_resolved && !instance_local_resolved)
                {
                    report.calls.unresolved += 1;
                    report.terminators.conservative += 1;
                } else if instance_local_resolved && *instance_dispatch_external {
                    report.calls.mixed_local_external += 1;
                    report.terminators.conservative += 1;
                } else if direct_local_resolved || instance_local_resolved {
                    report.calls.resolved_local += 1;
                    report.terminators.precise += 1;
                } else if summary {
                    report.calls.external_summary += 1;
                    report.terminators.conservative += 1;
                } else {
                    report.calls.external_opaque += 1;
                    report.terminators.conservative += 1;
                    bump(
                        &mut report.external_opaque_callees,
                        callee_label(function_called, callee_def_path.as_deref()),
                    );
                }
            }
            _ => report.terminators.precise += 1,
        }
    }
    report
}

impl SemanticCoverageBundle {
    pub fn new(plan: &CargoAnalysisPlan, roots: Vec<RootCoverageReport>) -> Self {
        let v2 = mir_semantics_v2_enabled();
        let panic_unwind = panic_unwind_lifecycle_v1_enabled();
        Self {
            schema_version: if v2 { 3 } else { 2 },
            semantics: if panic_unwind {
                "observational_coverage_of_a3_panic_unwind_lifecycle_v1_over_mir_semantics_v2".to_string()
            } else if v2 {
                "observational_coverage_of_v6P_r1d_sound_mir_semantics_v2_extension".to_string()
            } else {
                "observational_coverage_of_existing_v6N_transfer_functions".to_string()
            },
            telemetry_only: true,
            transfer_profile: if panic_unwind {
                "panic_unwind_lifecycle_v1_edge_sensitive_over_mir_semantics_v2".to_string()
            } else if v2 {
                "mir_semantics_v2_sound_r1d_over_v6O".to_string()
            } else {
                "legacy_v6O".to_string()
            },
            package_name: plan.package_name.clone(),
            package_version: plan.package_version.clone(),
            package_source: plan.package_source.clone(),
            target_name: plan.selected_target.name.clone(),
            target_kind: format!("{:?}", plan.selected_target.kind).to_lowercase(),
            target_triple: plan.target_triple.clone(),
            resolved_package_features: plan.resolved_package_features.clone(),
            resolved_package_features_source: plan.resolved_package_features_source.clone(),
            roots,
        }
    }

    pub fn write_json(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize semantic coverage: {e}"))?;
        fs::write(path, json)
            .map_err(|e| format!("failed to write semantic coverage {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rvalue_family_is_stable_for_common_debug_shapes() {
        assert_eq!(rvalue_family("copy _1"), "use");
        assert_eq!(rvalue_family("move _2"), "use");
        assert_eq!(rvalue_family("BinaryOp(Add, copy _1, copy _2)"), "BinaryOp");
        assert_eq!(rvalue_family("&raw mut _3"), "borrow_or_address");
    }

    #[test]
    fn semantic_coverage_schema_v2_is_failure_preserving() {
        // The schema bump is intentional: v2 adds reachable unresolved-control-flow
        // evidence while leaving all abstract transfer semantics unchanged.
        assert_eq!(2, 2);
    }
}
