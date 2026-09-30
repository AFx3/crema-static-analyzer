use crate::structs::{AbstractAllocId, PlaceId, PlaceProjection, ProgramVarId};
use crate::memory_events::{self, RustAllocationSemantics};
use crate::utils::load_ffi_functions;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExternalReturnContract {
    pub semantic_class: &'static str,
    pub relation_kind: &'static str,
    pub nullability: &'static str,
    pub ownership: &'static str,
    pub semantic_source: &'static str,
}

pub(crate) fn external_return_contract(
    callee: &str, arity: usize, represented_body: bool, exact_zero: bool,
    ffi: &HashSet<String>,
) -> Option<ExternalReturnContract> {
    let name = callee.trim();
    if represented_body || !ffi.contains(name) { return None; }
    let (semantic_class, relation_kind, semantic_source) = match (name, arity) {
        ("memcpy", 3) => ("memcpy_return_dst_v1", "exact_argument_alias", "posix_memcpy_returns_destination_v1"),
        ("memmove", 3) => ("memmove_return_dst_v1", "exact_argument_alias", "posix_memmove_returns_destination_v1"),
        ("memset", 3) => ("memset_return_dst_v1", "exact_argument_alias", "posix_memset_returns_destination_v1"),
        ("memchr", 3) if !exact_zero => ("memchr_return_derived_v1", "nullable_derived_alias", "posix_memchr_nullable_derived_return_v1"),
        ("strchr", 2) => ("strchr_return_derived_v1", "nullable_derived_alias", "posix_strchr_nullable_derived_return_v1"),
        ("getenv", 1) => ("getenv_borrowed_environment_v1", "nullable_borrowed_external", "posix_getenv_nullable_borrowed_environment_v1"),
        _ => return None,
    };
    Some(ExternalReturnContract {
        semantic_class, relation_kind, semantic_source,
        nullability: if relation_kind == "exact_argument_alias" { "same_as_source" } else { "nullable" },
        ownership: if name == "getenv" { "borrowed_external" } else { "alias_existing" },
    })
}

#[cfg(test)]
mod err1_tests {
    use super::*;

    #[test]
    fn err1_closed_classifier_and_zero_orthogonality() {
        let ffi = ["memcpy", "memmove", "memset", "memchr", "strchr", "getenv"]
            .into_iter().map(str::to_string).collect();
        for (name, arity, kind) in [
            ("memcpy", 3, "exact_argument_alias"), ("memmove", 3, "exact_argument_alias"),
            ("memset", 3, "exact_argument_alias"), ("memchr", 3, "nullable_derived_alias"),
            ("strchr", 2, "nullable_derived_alias"), ("getenv", 1, "nullable_borrowed_external"),
        ] {
            assert_eq!(external_return_contract(name, arity, false, false, &ffi).unwrap().relation_kind, kind);
            assert!(external_return_contract(name, arity + 1, false, false, &ffi).is_none());
            assert!(external_return_contract(name, arity, true, false, &ffi).is_none());
            assert!(external_return_contract(name, arity, false, false, &HashSet::new()).is_none());
        }
        for (name, arity) in [("my_memmove", 3), ("foo_strchr", 2), ("getenv_wrapper", 1), ("libc::memcpy", 3)] {
            assert!(external_return_contract(name, arity, false, false, &ffi).is_none());
        }
        for name in ["memcpy", "memmove", "memset"] {
            assert!(external_return_contract(name, 3, false, true, &ffi).is_some());
        }
        assert!(external_return_contract("memchr", 3, false, true, &ffi).is_none());
        assert!(external_return_contract("memchr", 3, false, false, &ffi).is_some());
    }

    #[test]
    fn err1_aliases_preserve_candidates_without_granting_derived_base_frees() {
        let ffi = ["memcpy", "memmove", "memset", "memchr", "strchr", "getenv"]
            .into_iter().map(str::to_string).collect();
        let source = local("main", "Local(_1)").unwrap();
        let result = local("main", "Local(_4)").unwrap();
        let allocs: BTreeSet<_> = ["a", "b"].into_iter().map(|node| AbstractAllocId::new(
            AllocationSiteId::RustCall { node_id: node.into(), callee: "Box::new".into() }, vec![]
        )).collect();
        let args = ["Local(_1)", "Local(_2)", "const 8_usize"].map(|arg| crate::structs::MirCallArgument { arg: arg.into(), is_mutable: None });
        for callee in ["memcpy", "memmove", "memset", "memchr", "strchr", "getenv"] {
            let mut memory = AllocationIdentityMemory::default();
            memory.assign_points_to(source.clone(), allocs.clone());
            let arity = if callee == "getenv" { 1 } else if callee == "strchr" { 2 } else { 3 };
            transfer_library_call_with_external_context("rust::main::bb0", "main", &[], callee, "", &args[..arity], "Local(_4)", false, false, &ffi, &mut memory);
            if ["memcpy", "memmove", "memset"].contains(&callee) {
                assert_eq!(memory.points_to(&result), allocs);
                assert_eq!(memory.deallocation_allocations(&result), allocs);
            } else if callee == "getenv" {
                assert!(memory.event_allocations(&result).is_empty());
            } else {
                assert_eq!(memory.event_allocations(&result), allocs);
                assert!(memory.points_to(&result).is_empty());
                assert!(memory.deallocation_allocations(&result).is_empty());
                // Exact-return equality and pointer copies preserve access-only provenance.
                let mut exact_args = args.clone(); exact_args[0].arg = "Local(_4)".into();
                transfer_library_call_with_external_context("rust::main::bb1", "main", &[], "memmove", "", &exact_args, "Local(_5)", false, false, &ffi, &mut memory);
                let exact = local("main", "Local(_5)").unwrap();
                assert_eq!(memory.event_allocations(&exact), allocs);
                assert!(memory.deallocation_allocations(&exact).is_empty());
                let copied = local("main", "Local(_6)").unwrap();
                copy_binding(&mut memory, &exact, copied.clone());
                assert_eq!(memory.event_allocations(&copied), allocs);
                assert!(memory.deallocation_allocations(&copied).is_empty());
            }
            let catalog: BTreeSet<_> = memory.points_to.values().chain(memory.access_bases.values()).flatten().cloned().collect();
            assert_eq!(catalog, allocs, "ERR1 cannot add allocations");
        }
    }

    #[test]
    fn err1_unknown_zero_and_interior_source_provenance() {
        let ffi = ["memmove", "memchr"].into_iter().map(str::to_string).collect();
        let source = local("main", "Local(_1)").unwrap();
        let result = local("main", "Local(_4)").unwrap();
        let allocation = AbstractAllocId::new(AllocationSiteId::RustCall { node_id:"a".into(), callee:"Box::new".into() }, vec![]);
        let mut memory = AllocationIdentityMemory::default();
        let args = ["Local(_1)", "Local(_2)", "const 0_usize"].map(|arg| crate::structs::MirCallArgument { arg:arg.into(), is_mutable:None });
        transfer_library_call_with_external_context("rust::main::bb0", "main", &[], "memmove", "", &args, "Local(_4)", false, false, &ffi, &mut memory);
        assert!(memory.event_allocations(&result).is_empty());
        memory.assign_fresh(source.clone(), allocation.clone());
        transfer_library_call_with_external_context("rust::main::bb0", "main", &[], "memmove", "", &args, "Local(_4)", false, false, &ffi, &mut memory);
        assert_eq!(memory.deallocation_allocations(&result), BTreeSet::from([allocation.clone()]));
        transfer_library_call_with_external_context("rust::main::bb0", "main", &[], "memchr", "", &args, "Local(_4)", false, false, &ffi, &mut memory);
        assert!(memory.event_allocations(&result).is_empty(), "zero clears stale result");
        transfer_library_call_with_external_context("rust::main::bb1", "main", &[], "std::ptr::mut_ptr::<impl *mut u8>::add", "", &args[..2], "Local(_1)", false, false, &ffi, &mut memory);
        assert!(memory.deallocation_allocations(&source).is_empty());
        transfer_library_call_with_external_context("rust::main::bb2", "main", &[], "memmove", "", &args, "Local(_4)", false, false, &ffi, &mut memory);
        assert_eq!(memory.event_allocations(&result), BTreeSet::from([allocation]));
        assert!(memory.deallocation_allocations(&result).is_empty(), "exact aliases cannot upgrade interiors");
    }
}

/// Additional implementation-level domain for canonical interprocedural
/// allocation identity.
///
/// This domain is intentionally separate from the formal `CellValue` lattice.
/// Phase 6A introduces and tests its algebra without yet changing CREMA's
/// legacy detector or fixed-point transfer functions.  Later phases populate
/// it from MIR/SVF and use it as the authoritative identity relation for CQPL.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AllocationIdentityMemory {
    /// MAY points-to relation.  One program variable may denote several
    /// abstract allocation IDs after control-flow joins.
    pub points_to: BTreeMap<ProgramVarId, BTreeSet<AbstractAllocId>>,
    /// ERR1 access-base association only. Never consulted to certify drops.
    #[serde(default)]
    pub access_bases: BTreeMap<ProgramVarId, BTreeSet<AbstractAllocId>>,

    /// MAY stack-place reference relation.  Kept distinct from heap points-to:
    /// `&x` denotes a stack place, not the allocation denoted by x.
    pub stack_refs: BTreeMap<ProgramVarId, BTreeSet<PlaceId>>,

    /// Value identity stored in projected MIR places (for example closure fields).
    /// Empty projections are represented by the variable maps above; these maps
    /// are only needed for non-empty projections.
    #[serde(default)]
    pub place_points_to: BTreeMap<PlaceId, BTreeSet<AbstractAllocId>>,
    #[serde(default)]
    pub place_stack_refs: BTreeMap<PlaceId, BTreeSet<PlaceId>>,
}

impl AllocationIdentityMemory {
    pub fn points_to(&self, var: &ProgramVarId) -> BTreeSet<AbstractAllocId> {
        self.points_to.get(var).cloned().unwrap_or_default()
    }

    pub fn stack_refs(&self, var: &ProgramVarId) -> BTreeSet<PlaceId> {
        self.stack_refs.get(var).cloned().unwrap_or_default()
    }

    pub fn points_to_place(&self, place: &PlaceId) -> BTreeSet<AbstractAllocId> {
        if place.projection.is_empty() {
            return self.points_to(&place.base);
        }
        self.place_points_to.get(place).cloned().unwrap_or_default()
    }

    pub fn stack_refs_place(&self, place: &PlaceId) -> BTreeSet<PlaceId> {
        if place.projection.is_empty() {
            return self.stack_refs(&place.base);
        }
        self.place_stack_refs.get(place).cloned().unwrap_or_default()
    }

    /// Resolve the abstract allocations denoted by an event subject.
    ///
    /// Memory-access labels may syntactically mention a reference temporary
    /// rather than the loaded pointer value itself (for example a
    /// `CopyForDeref` temporary in a closure).  For CQPL allocation-centric
    /// event correlation, follow the finite MAY stack-place relation until
    /// allocation identities are reached.  This does not manufacture alias
    /// equivalence: every returned allocation is justified by a points-to edge
    /// or by a stack-reference/place edge already present in this memory.
    pub fn allocations_for_place(&self, start: PlaceId) -> BTreeSet<AbstractAllocId> {
        self.allocations_for_place_kind(start, true)
    }

    fn allocations_for_place_kind(&self, start: PlaceId, access: bool) -> BTreeSet<AbstractAllocId> {
        let mut out = BTreeSet::new();
        let mut pending = vec![start];
        let mut seen = BTreeSet::new();

        while let Some(place) = pending.pop() {
            if !seen.insert(place.clone()) {
                continue;
            }

            out.extend(self.points_to_place(&place));
            if access && place.projection.is_empty() {
                out.extend(self.access_bases.get(&place.base).into_iter().flatten().cloned());
            }
            pending.extend(self.stack_refs_place(&place));

            // A projected place whose base is itself a reference denotes the
            // same projection below every already-proved referent of that base.
            // Example: `&mut self.container` in a callee where `self` is a
            // reference to the caller's aggregate.  Rebasing is justified only
            // by the existing stack-reference relation; it does not manufacture
            // heap aliases.
            for mut base_target in self.stack_refs(&place.base) {
                base_target.projection.extend(place.projection.iter().cloned());
                pending.push(base_target);
            }
        }

        out
    }

    pub fn event_allocations(&self, var: &ProgramVarId) -> BTreeSet<AbstractAllocId> {
        let mut out = self.points_to(var);
        out.extend(self.access_bases.get(var).into_iter().flatten().cloned());
        for place in self.stack_refs(var) {
            out.extend(self.allocations_for_place(place));
        }
        out
    }

    pub fn deallocation_allocations(&self, var: &ProgramVarId) -> BTreeSet<AbstractAllocId> {
        let mut out = self.points_to(var);
        for place in self.stack_refs(var) {
            out.extend(self.allocations_for_place_kind(place, false));
        }
        out
    }

    fn assign_access_bases(&mut self, var: ProgramVarId, allocs: BTreeSet<AbstractAllocId>) {
        if allocs.is_empty() { self.access_bases.remove(&var); }
        else { self.access_bases.insert(var, allocs); }
    }

    /// Strong overwrite of one variable's heap identity.
    pub fn assign_points_to(
        &mut self,
        var: ProgramVarId,
        allocs: BTreeSet<AbstractAllocId>,
    ) {
        self.access_bases.remove(&var);
        if allocs.is_empty() {
            self.points_to.remove(&var);
        } else {
            self.points_to.insert(var, allocs);
        }
    }

    /// Strong overwrite with one fresh abstract allocation identity.
    pub fn assign_fresh(&mut self, var: ProgramVarId, alloc: AbstractAllocId) {
        self.access_bases.remove(&var);
        self.points_to.insert(var, BTreeSet::from([alloc]));
    }

    /// Strong overwrite of one variable's stack-reference targets.
    pub fn assign_stack_refs(&mut self, var: ProgramVarId, places: BTreeSet<PlaceId>) {
        if places.is_empty() {
            self.stack_refs.remove(&var);
        } else {
            self.stack_refs.insert(var, places);
        }
    }

    pub fn assign_place_points_to(
        &mut self,
        place: PlaceId,
        allocs: BTreeSet<AbstractAllocId>,
    ) {
        debug_assert!(!place.projection.is_empty());
        if allocs.is_empty() {
            self.place_points_to.remove(&place);
        } else {
            self.place_points_to.insert(place, allocs);
        }
    }

    pub fn assign_place_stack_refs(&mut self, place: PlaceId, refs: BTreeSet<PlaceId>) {
        debug_assert!(!place.projection.is_empty());
        if refs.is_empty() {
            self.place_stack_refs.remove(&place);
        } else {
            self.place_stack_refs.insert(place, refs);
        }
    }

    pub fn forget_var(&mut self, var: &ProgramVarId) {
        self.access_bases.remove(var);
        self.points_to.remove(var);
        self.stack_refs.remove(var);
        self.place_points_to.retain(|place, _| &place.base != var);
        self.place_stack_refs.retain(|place, _| &place.base != var);
    }

    /// MAY alias is intersection of points-to sets, not equivalence closure.
    #[cfg(test)]
    pub fn may_alias(&self, left: &ProgramVarId, right: &ProgramVarId) -> bool {
        let l = self.points_to(left);
        let r = self.points_to(right);
        !l.is_disjoint(&r)
    }

    /// Return whether both variables have the same singleton MAY target set.
    ///
    /// This is intentionally *not* called must-alias: singleton cardinality in
    /// the finite abstraction does not imply a unique concrete allocation in
    /// every concretization (e.g. repeated allocations at one site/context).
    #[cfg(test)]
    pub fn same_singleton_abstract_id(&self, left: &ProgramVarId, right: &ProgramVarId) -> bool {
        let l = self.points_to(left);
        let r = self.points_to(right);
        l.len() == 1 && l == r
    }

    /// Pointwise MAY join.  This does not manufacture transitive aliases.
    pub fn join(&self, other: &Self) -> Self {
        let mut out = self.clone();
        for (var, allocs) in &other.access_bases {
            out.access_bases.entry(var.clone()).or_default().extend(allocs.iter().cloned());
        }

        for (var, allocs) in &other.points_to {
            out.points_to
                .entry(var.clone())
                .or_default()
                .extend(allocs.iter().cloned());
        }

        for (var, places) in &other.stack_refs {
            out.stack_refs
                .entry(var.clone())
                .or_default()
                .extend(places.iter().cloned());
        }

        for (place, allocs) in &other.place_points_to {
            out.place_points_to
                .entry(place.clone())
                .or_default()
                .extend(allocs.iter().cloned());
        }

        for (place, refs) in &other.place_stack_refs {
            out.place_stack_refs
                .entry(place.clone())
                .or_default()
                .extend(refs.iter().cloned());
        }

        out
    }

    /// Precision order for MAY components: subset means more precise.
    #[cfg(test)]
    pub fn leq(&self, other: &Self) -> bool {
        let points_to_leq = self.points_to.iter().all(|(var, allocs)| {
            allocs.is_subset(&other.points_to(var))
        });

        let refs_leq = self.stack_refs.iter().all(|(var, places)| {
            places.is_subset(&other.stack_refs(var))
        });

        let place_points_to_leq = self.place_points_to.iter().all(|(place, allocs)| {
            allocs.is_subset(&other.points_to_place(place))
        });

        let place_refs_leq = self.place_stack_refs.iter().all(|(place, refs)| {
            refs.is_subset(&other.stack_refs_place(place))
        });

        let access_leq = self.access_bases.iter().all(|(var, allocs)| {
            allocs.is_subset(&other.access_bases.get(var).cloned().unwrap_or_default())
        });
        points_to_leq && refs_leq && place_points_to_leq && place_refs_leq && access_leq
    }
}


// -----------------------------------------------------------------------------
// Phase 6C: context-sensitive interprocedural allocation-identity fixed point
// -----------------------------------------------------------------------------

use crate::structs::{
    AllocationSiteId, DummyNode, GlobalICFGNode, GlobalICFGOrdered, LlvmJsonNode,
    MirBasicBlock, MirStatement, MirTerminator, RustCallMetadata, SvfStatement,
};

/// One context-sensitive analysis point.  Phase 6C uses a bounded call string
/// of length one.  The bound makes the domain finite; the callsite element is
/// nevertheless sufficient to keep two distinct callers of the same callee
/// separate in the allocation-site abstraction.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IdentityAnalysisPoint {
    pub node: String,
    pub context: Vec<String>,
}

/// Result of the Phase-6C allocation-identity analysis.
///
/// `by_point` is the context-sensitive post-state.  `by_node` is its MAY join
/// over contexts and is the representation intended for later CQPL export.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AllocationIdentityState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_resource_identity_continuity_v1: Option<serde_json::Value>,
    /// Context-sensitive post-state after executing the whole ICFG node.
    pub by_point: BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory>,
    /// MAY join of `by_point` over bounded contexts.
    pub by_node: BTreeMap<String, AllocationIdentityMemory>,
    /// Context-sensitive intra-node event summary.  For each node/context this
    /// is the MAY join of identity memories observed at statement/terminator
    /// boundaries while executing the node.  Allocation-centric event labels
    /// must be resolved against this summary rather than only the final
    /// post-state, otherwise an overwrite later in the same basic block can
    /// erase the allocation identity of an earlier event.
    #[serde(default)]
    pub event_by_point: BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory>,
    /// MAY join of `event_by_point` over bounded contexts.
    #[serde(default)]
    pub event_by_node: BTreeMap<String, AllocationIdentityMemory>,
}


#[derive(Debug, Clone, Serialize)]
pub struct AllocationIdentityDump {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_resource_identity_continuity_v1: Option<serde_json::Value>,
    pub schema: &'static str,
    pub by_point: Vec<IdentityPointDump>,
    pub by_node: Vec<IdentityNodeDump>,
    pub event_by_point: Vec<IdentityPointDump>,
    pub event_by_node: Vec<IdentityNodeDump>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityPointDump {
    pub node: String,
    pub context: Vec<String>,
    pub memory: IdentityMemoryDump,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityNodeDump {
    pub node: String,
    pub memory: IdentityMemoryDump,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityMemoryDump {
    pub points_to: Vec<VariablePointsToDump>,
    pub stack_refs: Vec<VariableStackRefsDump>,
    pub place_points_to: Vec<PlacePointsToDump>,
    pub place_stack_refs: Vec<PlaceStackRefsDump>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VariablePointsToDump {
    pub variable: ProgramVarId,
    pub canonical_variable: String,
    pub allocations: Vec<AbstractAllocId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VariableStackRefsDump {
    pub variable: ProgramVarId,
    pub canonical_variable: String,
    pub places: Vec<PlaceId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlacePointsToDump {
    pub place: PlaceId,
    pub allocations: Vec<AbstractAllocId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaceStackRefsDump {
    pub place: PlaceId,
    pub targets: Vec<PlaceId>,
}

impl IdentityMemoryDump {
    fn from_memory(memory: &AllocationIdentityMemory) -> Self {
        Self {
            points_to: memory
                .points_to
                .iter()
                .map(|(variable, allocations)| VariablePointsToDump {
                    variable: variable.clone(),
                    canonical_variable: variable.canonical_string(),
                    allocations: allocations.iter().cloned().collect(),
                })
                .collect(),
            stack_refs: memory
                .stack_refs
                .iter()
                .map(|(variable, places)| VariableStackRefsDump {
                    variable: variable.clone(),
                    canonical_variable: variable.canonical_string(),
                    places: places.iter().cloned().collect(),
                })
                .collect(),
            place_points_to: memory
                .place_points_to
                .iter()
                .map(|(place, allocations)| PlacePointsToDump {
                    place: place.clone(),
                    allocations: allocations.iter().cloned().collect(),
                })
                .collect(),
            place_stack_refs: memory
                .place_stack_refs
                .iter()
                .map(|(place, targets)| PlaceStackRefsDump {
                    place: place.clone(),
                    targets: targets.iter().cloned().collect(),
                })
                .collect(),
        }
    }
}

impl AllocationIdentityState {
    pub fn to_dump(&self) -> AllocationIdentityDump {
        AllocationIdentityDump {
            dependency_resource_identity_continuity_v1: self.dependency_resource_identity_continuity_v1.clone(),
            schema: "CREMA Allocation Identity v6G",
            by_point: self
                .by_point
                .iter()
                .map(|(point, memory)| IdentityPointDump {
                    node: point.node.clone(),
                    context: point.context.clone(),
                    memory: IdentityMemoryDump::from_memory(memory),
                })
                .collect(),
            by_node: self
                .by_node
                .iter()
                .map(|(node, memory)| IdentityNodeDump {
                    node: node.clone(),
                    memory: IdentityMemoryDump::from_memory(memory),
                })
                .collect(),
            event_by_point: self
                .event_by_point
                .iter()
                .map(|(point, memory)| IdentityPointDump {
                    node: point.node.clone(),
                    context: point.context.clone(),
                    memory: IdentityMemoryDump::from_memory(memory),
                })
                .collect(),
            event_by_node: self
                .event_by_node
                .iter()
                .map(|(node, memory)| IdentityNodeDump {
                    node: node.clone(),
                    memory: IdentityMemoryDump::from_memory(memory),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct IdentityContinuation {
    caller_context: Vec<String>,
    caller_memory: AllocationIdentityMemory,
    initialized: bool,
}

impl IdentityContinuation {
    fn join_snapshot(
        &mut self,
        caller_context: &[String],
        memory: &AllocationIdentityMemory,
    ) -> bool {
        if !self.initialized {
            self.caller_context = caller_context.to_vec();
            self.caller_memory = memory.clone();
            self.initialized = true;
            return true;
        }

        // A context key includes the caller context, so reaching this branch
        // with a different context would be an implementation error.
        debug_assert_eq!(self.caller_context, caller_context);
        let joined = self.caller_memory.join(memory);
        let changed = joined != self.caller_memory;
        if changed {
            self.caller_memory = joined;
        }
        changed
    }
}

fn bounded_context(callsite: &str) -> Vec<String> {
    vec![callsite.to_string()]
}

fn rust_function_from_node(node_id: &str) -> Option<&str> {
    let rest = node_id.strip_prefix("rust::")?;
    let (function, block) = rest.rsplit_once("::bb")?;
    if function.is_empty() || block.parse::<usize>().is_err() {
        return None;
    }
    Some(function)
}

fn node_map(icfg: &GlobalICFGOrdered) -> BTreeMap<String, GlobalICFGNode> {
    icfg.ordered_nodes.iter().cloned().collect()
}

fn successor_map(icfg: &GlobalICFGOrdered) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &icfg.icfg_edges {
        out.entry(edge.source.clone())
            .or_default()
            .insert(edge.destination.clone());
    }
    out
}

fn has_rust_call_at(icfg: &GlobalICFGOrdered, node: &str) -> bool {
    // A generic MIR callsite may have 0..N concrete local branches after
    // v6L Instance resolution.  This predicate intentionally asks only
    // whether at least one internal Rust branch exists; branch propagation
    // itself is keyed by the unique dummyCall/dummyRet metadata below.
    icfg.rust_calls.iter().any(|call| call.call_node == node)
}

fn has_represented_external_body_at(icfg: &GlobalICFGOrdered, node: &str) -> bool {
    icfg.ordered_nodes.iter().any(|(_, candidate)| {
        matches!(
            candidate,
            GlobalICFGNode::DummyCall(dummy)
                if !dummy.is_internal.unwrap_or(false)
                    && dummy.incoming_edge == node
                    && dummy.outgoing_edge.starts_with("llvm::")
        )
    })
}

fn bodyless_c_allocator_name(
    callee: &str,
    call_text: &str,
    ffi_functions: &HashSet<String>,
) -> Option<&'static str> {
    let callee = callee.trim();
    let call_text = call_text.trim();

    let declared_malloc = ffi_functions.contains("malloc")
        && (callee == "malloc"
            || call_text == "malloc"
            || call_text.starts_with("malloc(")
            || call_text.contains(" malloc("));
    let declared_calloc = ffi_functions.contains("calloc")
        && (callee == "calloc"
            || call_text == "calloc"
            || call_text.starts_with("calloc(")
            || call_text.contains(" calloc("));
    let declared_strdup = ffi_functions.contains("strdup")
        && (callee == "strdup"
            || call_text == "strdup"
            || call_text.starts_with("strdup(")
            || call_text.contains(" strdup("));

    if declared_malloc
        || (callee.contains("libc::") && callee.ends_with("::malloc"))
        || (call_text.contains("libc::") && call_text.contains("::malloc("))
    {
        Some("malloc")
    } else if declared_calloc
        || (callee.contains("libc::") && callee.ends_with("::calloc"))
        || (call_text.contains("libc::") && call_text.contains("::calloc("))
    {
        Some("calloc")
    } else if declared_strdup
        || (callee.contains("libc::") && callee.ends_with("::strdup"))
        || (call_text.contains("libc::") && call_text.contains("::strdup("))
    {
        Some("strdup")
    } else {
        None
    }
}

/// Closed bodyless C realloc classifier used only after a separate proof that
/// formal 0 is definitely null.  Bare symbols remain foreign-declaration
/// gated; libc paths are admitted by the same explicit namespace convention
/// used by the existing bodyless allocation summaries.
fn is_bodyless_c_realloc_call(
    callee: &str,
    call_text: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    let callee = callee.trim();
    let call_text = call_text.trim();
    let declared = ffi_functions.contains("realloc")
        && (callee == "realloc"
            || call_text == "realloc"
            || call_text.starts_with("realloc(")
            || call_text.contains(" realloc("));
    let libc = (callee.contains("libc::") && callee.ends_with("::realloc"))
        || (call_text.contains("libc::") && call_text.contains("::realloc("));
    declared || libc
}

/// Exact rustc DefPath classifier for canonical null-pointer constructors.
/// This is a MUST-null proof source; pretty strings and arbitrary functions
/// named `null_mut` are deliberately insufficient.
fn is_null_pointer_constructor_def_path(path: Option<&str>) -> bool {
    let Some(path) = path.map(str::trim) else { return false; };
    matches!(path, "core::ptr::null_mut" | "std::ptr::null_mut")
}

/// Intraprocedural MUST-null dataflow used solely to prove the special C/POSIX
/// rule `realloc(NULL, n) == malloc(n)`.  The domain is an intersection domain:
/// a local is retained only when every represented incoming MIR path proves it
/// null.  Unsupported calls/control-flow simply lose the fact (fail closed).
fn definitely_null_at_mir_terminators(
    icfg: &GlobalICFGOrdered,
    entry: &str,
) -> BTreeMap<String, BTreeSet<ProgramVarId>> {
    let nodes = node_map(icfg);
    let succs = successor_map(icfg);
    let mut in_states: BTreeMap<String, BTreeSet<ProgramVarId>> = BTreeMap::new();
    let mut before_terminator: BTreeMap<String, BTreeSet<ProgramVarId>> = BTreeMap::new();
    let mut worklist = BTreeSet::from([entry.to_string()]);
    in_states.insert(entry.to_string(), BTreeSet::new());

    while let Some(node_id) = worklist.iter().next().cloned() {
        worklist.remove(&node_id);
        let Some(GlobalICFGNode::Mir(bb)) = nodes.get(&node_id) else { continue; };
        let Some(function) = rust_function_from_node(&node_id) else { continue; };
        let mut state = in_states.get(&node_id).cloned().unwrap_or_default();

        for stmt in &bb.statements {
            let Some(place) = stmt.place.as_deref() else { continue; };
            let Some(dest) = assignment_destination(
                function,
                place,
                IdentityTransferProfile::LegacyFrozen,
            ) else { continue; };
            if stmt.kind != "Assign" {
                // Storage/deinit/retag and other direct-place mutations are not
                // part of the closed RN1 propagation vocabulary.  Forget the
                // fact rather than retaining stale MUST-null information.
                state.remove(&dest);
                continue;
            }
            let source_is_null = stmt.rvalue.as_deref().is_some_and(|rvalue| {
                (rvalue.contains("copy ")
                    || rvalue.contains("move ")
                    || rvalue.contains(" as *")
                    || rvalue.contains("PointerCoercion"))
                    && first_local_operand(function, rvalue)
                        .is_some_and(|source| state.contains(&source))
            });
            state.remove(&dest);
            if source_is_null {
                state.insert(dest);
            }
        }

        before_terminator.insert(node_id.clone(), state.clone());

        match &bb.terminator {
            Some(MirTerminator::Call {
                callee_def_path,
                arguments,
                return_place,
                ..
            }) => {
                let is_null_constructor = arguments.is_empty()
                    && is_null_pointer_constructor_def_path(callee_def_path.as_deref());
                if !is_null_constructor {
                    // A general call may mutate a pointer local through an alias.
                    // Without a dedicated mod/ref proof, retaining any MUST-null
                    // fact across that boundary would be unsound.
                    state.clear();
                }
                if let Some(dest) = local(function, return_place) {
                    state.remove(&dest);
                    if is_null_constructor {
                        state.insert(dest);
                    }
                }
            }
            Some(MirTerminator::Goto { .. })
            | Some(MirTerminator::SwitchInt { .. })
            | Some(MirTerminator::Assert { .. })
            | Some(MirTerminator::FalseEdge { .. })
            | Some(MirTerminator::FalseUnwind { .. })
            | None => {}
            _ => {
                // Drop glue, inline asm, coroutine suspension, tail calls, and
                // other unsupported terminators may have effects outside this
                // tiny nullness vocabulary.  Forget rather than overclaim.
                state.clear();
            }
        }

        let Some(nexts) = succs.get(&node_id) else { continue; };
        for succ in nexts {
            let Some(GlobalICFGNode::Mir(_)) = nodes.get(succ) else { continue; };
            if rust_function_from_node(succ) != Some(function) {
                continue;
            }
            let changed = if let Some(existing) = in_states.get_mut(succ) {
                let joined: BTreeSet<_> = existing.intersection(&state).cloned().collect();
                if *existing != joined {
                    *existing = joined;
                    true
                } else {
                    false
                }
            } else {
                in_states.insert(succ.clone(), state.clone());
                true
            };
            if changed {
                worklist.insert(succ.clone());
            }
        }
    }

    before_terminator
}

fn rust_call_for_dummy_call<'a>(
    icfg: &'a GlobalICFGOrdered,
    node: &str,
) -> Option<&'a RustCallMetadata> {
    icfg.rust_calls.iter().find(|call| call.dummy_call_node == node)
}

fn rust_calls_for_callee<'a>(
    icfg: &'a GlobalICFGOrdered,
    callee: &str,
) -> Vec<&'a RustCallMetadata> {
    icfg.rust_calls
        .iter()
        .filter(|call| call.callee_function == callee)
        .collect()
}

fn local(function: &str, raw: &str) -> Option<ProgramVarId> {
    ProgramVarId::rust(function.to_string(), raw)
}

/// Return the destination variable only for a direct MIR-local assignment.
/// `describe_place` appends ` -> ...` for every non-empty MIR projection; a
/// write through such a place mutates projected storage, not the base local
/// that carries the pointer/owner identity.
fn direct_assignment_local(function: &str, place: &str) -> Option<ProgramVarId> {
    if place.contains(" -> ") {
        return None;
    }
    local(function, place)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdentityTransferProfile {
    LegacyFrozen,
    DispositionV6S,
}

fn assignment_destination(
    function: &str,
    place: &str,
    profile: IdentityTransferProfile,
) -> Option<ProgramVarId> {
    match profile {
        IdentityTransferProfile::LegacyFrozen => local(function, place),
        IdentityTransferProfile::DispositionV6S => direct_assignment_local(function, place),
    }
}

fn first_local_operand(function: &str, text: &str) -> Option<ProgramVarId> {
    // Prefer the canonical `Local(_N)` spelling emitted by the CREMA census.
    if let Some(pos) = text.find("Local(_") {
        return local(function, &text[pos..]);
    }

    // MIR rvalues stored in GlobalICFG use spellings such as `copy _4`,
    // `move _7`, `&_2`, and `copy (*_8)`.
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'_' {
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 {
            return local(function, &text[i..j]);
        }
    }
    None
}

fn direct_deref_reference(function: &str, rvalue: &str) -> Option<ProgramVarId> {
    let marker = "(*_";
    let pos = rvalue.find(marker)?;
    let start = pos + 2; // points to `_`
    let tail = &rvalue[start..];
    local(function, tail)
}

fn reference_field_projection(rvalue: &str, local_text: &str) -> Vec<PlaceProjection> {
    let Some(start) = rvalue.find(local_text) else {
        return Vec::new();
    };
    let suffix = &rvalue[start + local_text.len()..];
    let suffix = suffix.split(':').next().unwrap_or(suffix);
    let bytes = suffix.as_bytes();
    let mut projection = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'.' {
            i += 1;
            continue;
        }
        i += 1;
        let begin = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if begin == i {
            continue;
        }
        if let Ok(index) = suffix[begin..i].parse::<u32>() {
            projection.push(PlaceProjection::Field { index });
        }
    }
    projection
}

fn direct_reference_place(function: &str, rvalue: &str) -> Option<PlaceId> {
    let trimmed = rvalue.trim();
    if !trimmed.starts_with('&') || trimmed.starts_with("&raw") {
        return None;
    }
    let base = first_local_operand(function, trimmed)?;
    let local_text = match &base {
        ProgramVarId::Rust { local, .. } => format!("_{local}"),
        _ => return None,
    };
    Some(PlaceId {
        base,
        projection: reference_field_projection(trimmed, &local_text),
    })
}

fn aggregate_field_operands(function: &str, rvalue: &str) -> Option<Vec<(usize, ProgramVarId)>> {
    let text = rvalue.trim();

    // Address-of rvalues are MIR references, not aggregate constructors.  In
    // particular, spellings such as
    // `&mut ((*_1).0: std::mem::ManuallyDrop<std::boxed::Box<[T]>>)` contain
    // both `::` and a trailing `)`, which would otherwise satisfy the broad
    // tuple/ADT fallback below and prevent `direct_reference_place` from
    // recording the projected stack-place relation.  Reject the reference
    // family here rather than teaching lifecycle analysis about any concrete
    // container type.
    if text.starts_with('&')
        || text.starts_with("move ")
        || text.starts_with("copy ")
        || text.starts_with("deref_copy ")
    {
        return None;
    }

    let fields = if let Some(open) = text.rfind(" { ") {
        text.get(open + 3..)?.strip_suffix('}')?
    } else if text.starts_with('(') && text.ends_with(')') {
        &text[1..text.len() - 1]
    } else if text.contains("::") && text.ends_with(')') {
        let open = text.rfind('(')?;
        &text[open + 1..text.len() - 1]
    } else {
        return None;
    };

    let mut out = Vec::new();
    for (index, field) in fields.split(',').enumerate() {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let rhs = field.split_once(':').map(|(_, rhs)| rhs.trim()).unwrap_or(field);
        if let Some(source) = first_local_operand(function, rhs) {
            out.push((index, source));
        }
    }
    Some(out)
}

fn downcast_field_use(function: &str, rvalue: &str) -> Option<(ProgramVarId, Vec<PlaceProjection>)> {
    let text = rvalue.trim();
    if !(text.starts_with("copy ") || text.starts_with("move ")) || !text.contains(" as ") {
        return None;
    }
    let source = first_local_operand(function, text)?;
    let local_text = match &source {
        ProgramVarId::Rust { local, .. } => format!("_{local}"),
        _ => return None,
    };
    let projection = reference_field_projection(text, &local_text);
    (!projection.is_empty()).then_some((source, projection))
}

fn closure_capture_operands(function: &str, rvalue: &str) -> Option<Vec<ProgramVarId>> {
    if !rvalue.contains("closure@") {
        return None;
    }
    let body_start = rvalue.rfind("} {")? + 3;
    let body_end = rvalue.rfind('}')?;
    if body_end <= body_start {
        return None;
    }

    let mut captures = Vec::new();
    for field in rvalue[body_start..body_end].split(',') {
        let (_, operand) = field.split_once(':')?;
        captures.push(first_local_operand(function, operand.trim())?);
    }
    Some(captures)
}

fn deref_copy_projection(function: &str, rvalue: &str) -> Option<(ProgramVarId, Vec<PlaceProjection>)> {
    if !rvalue.trim_start().starts_with("deref_copy") {
        return None;
    }
    let reference = direct_deref_reference(function, rvalue)?;
    let marker = "(*_";
    let start = rvalue.find(marker)?;
    let close = rvalue[start..].find(')')? + start;
    let suffix = &rvalue[close + 1..];
    let suffix = suffix.split(':').next().unwrap_or(suffix);

    let mut projection = Vec::new();
    for component in suffix.split('.') {
        let component = component.trim();
        if component.is_empty() {
            continue;
        }
        let digits: String = component.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            break;
        }
        projection.push(PlaceProjection::Field {
            index: digits.parse().ok()?,
        });
    }
    Some((reference, projection))
}

fn projected_places(
    memory: &AllocationIdentityMemory,
    reference: &ProgramVarId,
    projection: &[PlaceProjection],
) -> BTreeSet<PlaceId> {
    memory
        .stack_refs(reference)
        .into_iter()
        .map(|mut place| {
            place.projection.extend(projection.iter().cloned());
            place
        })
        .collect()
}

/// Return true when `callee` ends in the named method, allowing rustc's
/// observed method-level turbofish spelling (for example
/// `...::cast::<c_void>` or `...::new::<&str>`).
///
/// This helper is intentionally lexical and local: callers still constrain
/// the receiver family before treating a call as allocation-identity
/// preserving.  It must not be used as a generic alias rule.
fn method_terminal(callee: &str, method: &str) -> bool {
    let plain = format!("::{method}");
    if callee.ends_with(&plain) {
        return true;
    }
    let generic = format!("::{method}::<");
    callee
        .rfind(&generic)
        .map(|index| {
            let suffix = &callee[index + generic.len()..];
            !suffix.is_empty() && suffix.ends_with('>')
        })
        .unwrap_or(false)
}

/// Fresh Rust allocation sites represented by the identity analysis.
///
/// `CString::new::<&str>` is deliberately narrow.  For an `&str` input the
/// normal-success path creates owned CString storage, so a successful concrete
/// allocation is represented by this site.  We do *not* classify arbitrary
/// `CString::new<T>` calls as fresh because inputs such as `Vec<u8>` may
/// transfer/reuse pre-existing storage instead of creating a fresh allocation.
/// Since identity is MAY information, the site may also be present on the
/// abstract exceptional/error join; that can lose precision but cannot invent
/// MUST certainty.
#[cfg(test)]
fn is_fresh_rust_allocator(callee: &str) -> bool {
    matches!(
        memory_events::rust_allocation_semantics(callee),
        RustAllocationSemantics::Fresh
    )
}

/// Calls whose normal return preserves the represented concrete allocation.
///
/// This is a direct source-to-destination MAY flow relation, not alias closure.
/// Each listed operation either reconstructs/borrows the same allocation or
/// performs address-preserving pointer arithmetic/casting.
fn is_identity_preserving_pointer_call(callee: &str) -> bool {
    let typed_terminal = |family: &str, method: &str| {
        callee.contains(family) && method_terminal(callee, method)
    };
    let pointer_family = callee.contains("std::ptr::")
        || callee.contains("core::ptr::")
        || callee.contains("NonNull");

    typed_terminal("Box", "into_raw")
        || typed_terminal("Box", "from_raw")
        || typed_terminal("CString", "into_raw")
        || typed_terminal("CString", "from_raw")
        || typed_terminal("CStr", "from_ptr")
        || typed_terminal("Vec", "from_raw_parts")
        || typed_terminal("String", "from_raw_parts")
        || method_terminal(callee, "as_ptr")
        || method_terminal(callee, "as_mut_ptr")
        || (pointer_family && method_terminal(callee, "cast"))
        || (pointer_family && method_terminal(callee, "wrapping_offset"))
        || (pointer_family && method_terminal(callee, "offset"))
        || (pointer_family && method_terminal(callee, "add"))
        || (pointer_family && method_terminal(callee, "sub"))
        || memory_events::is_into_vec_transfer_call(callee)
}

/// Normal-return extraction of the owned CString from exactly the
/// `Result<CString, NulError>` produced by `CString::new`.  On a normal return
/// from `unwrap`/`expect` the Result is necessarily `Ok(CString)`, hence the
/// contained allocation identity is preserved.  The matcher is intentionally
/// type-specific: arbitrary `Result<T,E>` extraction is not an identity rule.
fn is_cstring_result_extractor(callee: &str) -> bool {
    callee.contains("Result::<std::ffi::CString, std::ffi::NulError>")
        && (method_terminal(callee, "unwrap") || method_terminal(callee, "expect"))
}


fn is_manually_drop_new(callee: &str) -> bool {
    callee.contains("ManuallyDrop") && method_terminal(callee, "new")
}

fn is_manually_drop_take(callee: &str) -> bool {
    callee.contains("ManuallyDrop") && method_terminal(callee, "take")
}

fn is_result_try_branch(callee: &str) -> bool {
    callee.contains("std::result::Result")
        && callee.contains("std::ops::Try")
        && method_terminal(callee, "branch")
}

fn is_result_value_extractor(callee: &str) -> bool {
    callee.contains("std::result::Result")
        && (method_terminal(callee, "unwrap") || method_terminal(callee, "expect"))
}

// -----------------------------------------------------------------------------
// Phase 6E: modeled Rust <-> C/SVF allocation-identity bridge
// -----------------------------------------------------------------------------

/// Parse the function and replicated Rust callsite encoded in a global LLVM
/// ICFG node id such as `llvm::c_alloc_i32::node42::rust::main::bb3`.
fn llvm_scope_from_node_id(node_id: &str) -> Option<(String, String)> {
    let rest = node_id.strip_prefix("llvm::")?;
    let (function, tail) = rest.split_once("::node")?;
    let (_, callsite_tail) = tail.split_once("::rust::")?;
    if function.is_empty() || callsite_tail.is_empty() {
        return None;
    }
    Some((function.to_string(), format!("rust::{callsite_tail}")))
}

fn c_var(
    function: &str,
    var_id: usize,
    callsite: &str,
) -> ProgramVarId {
    ProgramVarId::c(
        function.to_string(),
        var_id,
        Some(callsite.to_string()),
    )
}

fn c_var_from_dummy_text(
    function: &str,
    raw: &str,
    fallback_callsite: &str,
) -> Option<ProgramVarId> {
    let (id_text, callsite) = match raw.split_once('@') {
        Some((id, suffix)) if !suffix.is_empty() => (id, suffix),
        _ => (raw, fallback_callsite),
    };
    let var_id = id_text.trim().trim_start_matches('%').parse::<usize>().ok()?;
    Some(c_var(function, var_id, callsite))
}

fn c_allocator_name(info: &str) -> Option<&'static str> {
    if info.contains("@malloc(") {
        Some("malloc")
    } else if info.contains("@calloc(") {
        Some("calloc")
    } else {
        None
    }
}

/// Structured SVF relations that may carry the same pointer/allocation value.
///
/// This deliberately mirrors the Phase-5 positive provenance flow and excludes
/// arithmetic/comparison results.  The relation is MAY: Phi/Select inputs are
/// unioned; no equivalence closure is manufactured.
fn svf_identity_flow_sources(stmt: &SvfStatement) -> Vec<usize> {
    match stmt.stmt_type.as_str() {
        "AssignStmt"
        | "CopyStmt"
        | "LoadStmt"
        | "StoreStmt"
        | "GepStmt"
        | "PhiStmt"
        | "SelectStmt" => {
            let mut out = Vec::new();
            if let Some(rhs) = stmt.rhs_var_id {
                out.push(rhs);
            }
            for operand in stmt.normalized_operand_var_ids() {
                if !out.contains(&operand) {
                    out.push(operand);
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

fn transfer_llvm_identity(
    node_id: &str,
    llvm_node: &LlvmJsonNode,
    context: &[String],
    input: &AllocationIdentityMemory,
) -> (AllocationIdentityMemory, AllocationIdentityMemory) {
    let Some((function, callsite)) = llvm_scope_from_node_id(node_id) else {
        return (input.clone(), input.clone());
    };
    let mut out = input.clone();
    let mut event_summary = input.clone();

    // The pinned SVF producer represents malloc/calloc's returned pointer by
    // an AddrStmt lhs on the allocator FunCallBlock.  That lhs receives a fresh
    // abstract allocation identity for this replicated FFI callsite.
    if llvm_node.node_kind_string == "FunCallBlock" {
        if let Some(allocator) = c_allocator_name(&llvm_node.info) {
            for result in llvm_node
                .svf_statements
                .iter()
                .filter(|stmt| stmt.stmt_type == "AddrStmt")
                .filter_map(|stmt| stmt.lhs_var_id)
            {
                event_summary = event_summary.join(&out);
                let dest = c_var(&function, result, &callsite);
                out.assign_fresh(
                    dest,
                    AbstractAllocId::new(
                        AllocationSiteId::CCall {
                            node_id: node_id.to_string(),
                            allocator: allocator.to_string(),
                        },
                        context.to_vec(),
                    ),
                );
                event_summary = event_summary.join(&out);
            }
        }
    }

    for stmt in &llvm_node.svf_statements {
        // Preserve both sides of each SVF statement.  Raw event labels are
        // block-level, so their allocation target is conservatively resolved
        // against the union of identities visible anywhere in the node.
        event_summary = event_summary.join(&out);
        let Some(lhs) = stmt.result_var_id() else { continue; };
        let sources = svf_identity_flow_sources(stmt);
        if sources.is_empty() {
            continue;
        }

        let mut allocs = BTreeSet::new();
        let mut refs = BTreeSet::new();
        for source in sources {
            let source = c_var(&function, source, &callsite);
            allocs.extend(out.points_to(&source));
            refs.extend(out.stack_refs(&source));
        }

        let dest = c_var(&function, lhs, &callsite);
        out.assign_points_to(dest.clone(), allocs);
        out.assign_stack_refs(dest, refs);
        event_summary = event_summary.join(&out);
    }

    event_summary = event_summary.join(&out);
    (out, event_summary)
}

fn transfer_external_dummy_call_identity(
    dummy: &DummyNode,
    input: &AllocationIdentityMemory,
) -> AllocationIdentityMemory {
    if dummy.is_internal.unwrap_or(false) {
        return input.clone();
    }

    let mut out = input.clone();
    let Some(caller_function) = rust_function_from_node(&dummy.incoming_edge) else {
        return out;
    };
    let Some((c_function, callsite)) = llvm_scope_from_node_id(&dummy.outgoing_edge) else {
        return out;
    };

    if !dummy.argument_bindings.is_empty() {
        for binding in &dummy.argument_bindings {
            let Some(source) = local(caller_function, &binding.mir_var) else {
                continue;
            };
            let Some(destination) =
                c_var_from_dummy_text(&c_function, &binding.llvm_var, &callsite)
            else {
                continue;
            };
            // MAY-copy one certified positional pair only. Different arguments
            // and different replicated callsites remain distinct ProgramVarIds.
            copy_binding(&mut out, &source, destination);
        }
        return out;
    }

    let (Some(mir_var), Some(llvm_var)) = (&dummy.mir_var, &dummy.llvm_var) else {
        return out;
    };
    let Some(source) = local(caller_function, mir_var) else { return out; };
    let Some(destination) = c_var_from_dummy_text(&c_function, llvm_var, &callsite) else {
        return out;
    };

    copy_binding(&mut out, &source, destination);
    out
}

fn transfer_external_dummy_ret_identity(
    dummy: &DummyNode,
    input: &AllocationIdentityMemory,
) -> AllocationIdentityMemory {
    if dummy.is_internal.unwrap_or(false) {
        return input.clone();
    }

    let mut out = input.clone();
    let (Some(mir_var), Some(llvm_var)) = (&dummy.mir_var, &dummy.llvm_var) else {
        return out;
    };
    let Some((c_function, callsite)) = llvm_scope_from_node_id(&dummy.incoming_edge) else {
        return out;
    };
    let Some(caller_function) = rust_function_from_node(&dummy.outgoing_edge) else {
        return out;
    };
    let Some(source) = c_var_from_dummy_text(&c_function, llvm_var, &callsite) else {
        return out;
    };
    let Some(destination) = local(caller_function, mir_var) else { return out; };

    copy_binding(&mut out, &source, destination);
    out
}

fn clear_destination(memory: &mut AllocationIdentityMemory, dest: &ProgramVarId) {
    memory.forget_var(dest);
}

#[cfg(test)]
fn transfer_statement(
    function: &str,
    stmt: &MirStatement,
    memory: &mut AllocationIdentityMemory,
) {
    transfer_statement_with_profile(
        function,
        stmt,
        memory,
        IdentityTransferProfile::LegacyFrozen,
    );
}

fn transfer_statement_with_profile(
    function: &str,
    stmt: &MirStatement,
    memory: &mut AllocationIdentityMemory,
    profile: IdentityTransferProfile,
) {
    if stmt.kind != "Assign" {
        return;
    }

    let Some(place) = stmt.place.as_deref() else { return; };
    let Some(dest) = assignment_destination(function, place, profile) else { return; };
    let Some(rvalue) = stmt.rvalue.as_deref() else {
        clear_destination(memory, &dest);
        return;
    };

    if let Some(captures) = closure_capture_operands(function, rvalue) {
        clear_destination(memory, &dest);
        for (index, source) in captures.into_iter().enumerate() {
            copy_value_into_field(memory, &source, &dest, index);
        }
        return;
    }

    if let Some(fields) = aggregate_field_operands(function, rvalue) {
        clear_destination(memory, &dest);
        for (index, source) in fields {
            copy_value_into_field(memory, &source, &dest, index);
        }
        return;
    }

    if let Some((source, projection)) = downcast_field_use(function, rvalue) {
        copy_downcast_payload(memory, &source, &projection, &dest);
        return;
    }

    if let Some((reference, projection)) = deref_copy_projection(function, rvalue) {
        let mut allocs = BTreeSet::new();
        let mut refs = BTreeSet::new();
        for place in projected_places(memory, &reference, &projection) {
            allocs.extend(memory.points_to_place(&place));
            refs.extend(memory.stack_refs_place(&place));
        }
        clear_destination(memory, &dest);
        memory.assign_points_to(dest.clone(), allocs);
        memory.assign_stack_refs(dest, refs);
        return;
    }

    if let Some(target) = direct_reference_place(function, rvalue) {
        clear_destination(memory, &dest);
        memory.assign_stack_refs(dest, BTreeSet::from([target]));
        return;
    }

    if let Some(reference) = direct_deref_reference(function, rvalue) {
        let mut allocs = BTreeSet::new();
        let mut refs = BTreeSet::new();
        for place in memory.stack_refs(&reference) {
            allocs.extend(memory.points_to_place(&place));
            refs.extend(memory.stack_refs_place(&place));
        }
        clear_destination(memory, &dest);
        memory.assign_points_to(dest.clone(), allocs);
        memory.assign_stack_refs(dest, refs);
        return;
    }

    if rvalue.contains("copy ")
        || rvalue.contains("move ")
        || rvalue.contains(" as *")
        || rvalue.contains("PointerCoercion")
    {
        if let Some(source) = first_local_operand(function, rvalue) {
            clear_destination(memory, &dest);
            copy_binding(memory, &source, dest);
            return;
        }
    }

    // Unknown assignments are strong overwrites for identity components.  This
    // is conservative because no heap identity is invented for an unsupported
    // rvalue; supported pointer-producing forms above preserve their IDs.
    clear_destination(memory, &dest);
}

fn transfer_library_call(
    node_id: &str,
    function: &str,
    context: &[String],
    callee: &str,
    arguments: &[crate::structs::MirCallArgument],
    return_place: &str,
    memory: &mut AllocationIdentityMemory,
) {
    transfer_library_call_with_external_context(
        node_id,
        function,
        context,
        callee,
        "",
        arguments,
        return_place,
        false,
        false,
        &HashSet::new(),
        memory,
    );
}

fn transfer_library_call_with_external_context(
    node_id: &str,
    function: &str,
    context: &[String],
    callee: &str,
    call_text: &str,
    arguments: &[crate::structs::MirCallArgument],
    return_place: &str,
    represented_external_body: bool,
    source_definitely_null: bool,
    ffi_functions: &HashSet<String>,
    memory: &mut AllocationIdentityMemory,
) {
    let Some(dest) = local(function, return_place) else { return; };

    if let Some(contract) = external_return_contract(
        callee, arguments.len(), represented_external_body,
        arguments.get(2).is_some_and(|arg| arg.arg.trim() == "const 0_usize"),
        ffi_functions,
    ) {
        let Some(dest) = direct_assignment_local(function, return_place) else { return; };
        let source = arguments.first().and_then(|arg| direct_assignment_local(function, &arg.arg));
        match contract.relation_kind {
            "exact_argument_alias" => {
                if let Some(source) = source {
                    let snapshot = memory.clone();
                    clear_destination(memory, &dest);
                    memory.assign_points_to(dest.clone(), snapshot.points_to(&source));
                    memory.assign_stack_refs(dest.clone(), snapshot.stack_refs(&source));
                    memory.assign_access_bases(dest, snapshot.access_bases.get(&source).cloned().unwrap_or_default());
                } else { clear_destination(memory, &dest); }
            }
            "nullable_derived_alias" => {
                let bases = source.as_ref().map(|source| memory.event_allocations(source)).unwrap_or_default();
                clear_destination(memory, &dest);
                memory.assign_access_bases(dest, bases);
            }
            "nullable_borrowed_external" => clear_destination(memory, &dest),
            _ => unreachable!(),
        }
        return;
    }
    // A zero-byte memchr has no positive result association, including stale bindings.
    if !represented_external_body && callee.trim() == "memchr" && ffi_functions.contains("memchr")
        && arguments.len() == 3 && arguments[2].arg.trim() == "const 0_usize" {
        clear_destination(memory, &dest);
        return;
    }

    // Phase B-minimal: a bodyless foreign malloc/calloc/strdup call has no LLVM/SVF
    // AddrStmt from which Phase 6E could derive a CCall identity.  Materialize
    // the same finite allocation-site abstraction at the real MIR callsite.
    // Bare names are accepted only when certified by the extracted foreign
    // declaration set.  If a represented LLVM body exists, that body remains
    // authoritative and this MIR-side fallback is suppressed.
    if !represented_external_body {
        if let Some(allocator) = bodyless_c_allocator_name(callee, call_text, ffi_functions) {
            clear_destination(memory, &dest);
            memory.assign_fresh(
                dest,
                AbstractAllocId::new(
                    AllocationSiteId::CCall {
                        node_id: node_id.to_string(),
                        allocator: allocator.to_string(),
                    },
                    context.to_vec(),
                ),
            );
            return;
        }

        // RN1: C/POSIX realloc with a MUST-null source is malloc-like.  This is
        // deliberately distinct from ordinary realloc identity: there is no old
        // allocation to invalidate or join with, so the returned non-null object
        // is one fresh c_malloc-family site.  Nullable failure remains encoded by
        // the MAY identity relation plus TOP lifecycle state in CQPL export.
        if source_definitely_null && is_bodyless_c_realloc_call(callee, call_text, ffi_functions) {
            clear_destination(memory, &dest);
            memory.assign_fresh(
                dest,
                AbstractAllocId::new(
                    AllocationSiteId::CCall {
                        node_id: node_id.to_string(),
                        allocator: "realloc".to_string(),
                    },
                    context.to_vec(),
                ),
            );
            return;
        }
    }

    if is_manually_drop_new(callee) {
        if let Some(source) = arguments
            .first()
            .and_then(|arg| first_local_operand(function, &arg.arg))
        {
            clear_destination(memory, &dest);
            copy_binding(memory, &source, dest);
        }
        return;
    }

    if is_manually_drop_take(callee) {
        if let Some(source) = arguments
            .first()
            .and_then(|arg| first_local_operand(function, &arg.arg))
        {
            let allocs = memory.deallocation_allocations(&source);
            let access: BTreeSet<_> = memory.event_allocations(&source).difference(&allocs).cloned().collect();
            clear_destination(memory, &dest);
            memory.assign_points_to(dest.clone(), allocs);
            memory.assign_access_bases(dest, access);
        }
        return;
    }

    if is_result_try_branch(callee) {
        if let Some(source) = arguments
            .first()
            .and_then(|arg| first_local_operand(function, &arg.arg))
        {
            clear_destination(memory, &dest);
            // Result::{Ok,Err}(x) and ControlFlow::{Continue,Break}(x) both
            // expose their payload as field 0 after the corresponding MIR
            // downcast. Preserve only already represented projected identity.
            copy_projected_subtree(
                memory,
                &source,
                &[PlaceProjection::Field { index: 0 }],
                &dest,
                &[PlaceProjection::Field { index: 0 }],
            );
        }
        return;
    }

    if is_result_value_extractor(callee) && !is_cstring_result_extractor(callee) {
        if let Some(source) = arguments
            .first()
            .and_then(|arg| first_local_operand(function, &arg.arg))
        {
            clear_destination(memory, &dest);
            // On a normal return unwrap/expect has selected Result::Ok.  Rebase
            // the already-proved Ok payload subtree onto the returned value.
            copy_projected_subtree(
                memory,
                &source,
                &[PlaceProjection::Field { index: 0 }],
                &dest,
                &[],
            );
        }
        return;
    }

    match memory_events::rust_allocation_semantics(callee) {
        RustAllocationSemantics::Fresh => {
            clear_destination(memory, &dest);
            memory.assign_fresh(
                dest,
                AbstractAllocId::new(
                    AllocationSiteId::RustCall {
                        node_id: node_id.to_string(),
                        callee: callee.to_string(),
                    },
                    context.to_vec(),
                ),
            );
            return;
        }
        RustAllocationSemantics::MayFreshOrTransfer => {
            // CString::new and similar summaries can either allocate new
            // storage or reuse/consume storage supplied by an Into<Vec<u8>>
            // argument. Preserve both MAY alternatives; never strong-update a
            // source identity away merely to manufacture a fresh site.
            let source = arguments
                .first()
                .and_then(|arg| first_local_operand(function, &arg.arg));
            let mut allocs = source
                .as_ref()
                .map(|src| memory.points_to(src))
                .unwrap_or_default();
            let refs = source
                .as_ref()
                .map(|src| memory.stack_refs(src))
                .unwrap_or_default();
            allocs.insert(AbstractAllocId::new(
                AllocationSiteId::RustCall {
                    node_id: node_id.to_string(),
                    callee: callee.to_string(),
                },
                context.to_vec(),
            ));
            clear_destination(memory, &dest);
            memory.assign_points_to(dest.clone(), allocs);
            memory.assign_stack_refs(dest, refs);
            return;
        }
        RustAllocationSemantics::Transfer => {
            if let Some(source) = arguments
                .first()
                .and_then(|arg| first_local_operand(function, &arg.arg))
            {
                let allocs = memory.points_to(&source);
                let refs = memory.stack_refs(&source);
                let access = memory.access_bases.get(&source).cloned().unwrap_or_default();
                clear_destination(memory, &dest);
                memory.assign_points_to(dest.clone(), allocs);
                memory.assign_stack_refs(dest.clone(), refs);
                memory.assign_access_bases(dest, access);
            }
            return;
        }
        RustAllocationSemantics::ConditionalReallocation => {
            // Rust `GlobalAlloc::realloc` has two observable outcomes:
            //
            // * non-null: ownership of the old block is transferred and the
            //   returned pointer is the only valid handle; the block may have
            //   moved or may have remained in place;
            // * null: ownership is not transferred and the original block is
            //   unchanged.
            //
            // AllocationIdentityMemory is a MAY domain, so the result points
            // to both the old abstract allocation (in-place/failure relation)
            // and a fresh call-site identity (moved-success relation).  The
            // CellValue transfer separately widens the old handle because this
            // identity relation alone does not encode pointer validity.
            let source = arguments
                .first()
                .and_then(|arg| first_local_operand(function, &arg.arg));
            let mut allocs = source
                .as_ref()
                .map(|src| memory.points_to(src))
                .unwrap_or_default();
            let refs = source
                .as_ref()
                .map(|src| memory.stack_refs(src))
                .unwrap_or_default();
            allocs.insert(AbstractAllocId::new(
                AllocationSiteId::RustCall {
                    node_id: node_id.to_string(),
                    callee: callee.to_string(),
                },
                context.to_vec(),
            ));
            clear_destination(memory, &dest);
            memory.assign_points_to(dest.clone(), allocs);
            memory.assign_stack_refs(dest, refs);
            return;
        }
        RustAllocationSemantics::None => {}
    }

    if is_identity_preserving_pointer_call(callee) || is_cstring_result_extractor(callee) {
        if let Some(source) = arguments
            .first()
            .and_then(|arg| first_local_operand(function, &arg.arg))
        {
            let allocs = memory.points_to(&source);
            let refs = memory.stack_refs(&source);
            let mut access = memory.access_bases.get(&source).cloned().unwrap_or_default();
            // Existing offset summaries preserve allocation-base association,
            // but no offset operation is a base-pointer validity certificate.
            let offset = ["wrapping_offset", "offset", "add", "sub"].iter().any(|method| method_terminal(callee, method));
            let allocs = if offset { access.extend(memory.event_allocations(&source)); BTreeSet::new() } else { allocs };
            let refs = if offset { BTreeSet::new() } else { refs };
            clear_destination(memory, &dest);
            memory.assign_points_to(dest.clone(), allocs);
            memory.assign_stack_refs(dest.clone(), refs);
            memory.assign_access_bases(dest, access);
        }
    }

}

fn transfer_mir_node(
    node_id: &str,
    bb: &MirBasicBlock,
    context: &[String],
    internal_call: bool,
    represented_external_body: bool,
    definitely_null_at_terminator: &BTreeSet<ProgramVarId>,
    ffi_functions: &HashSet<String>,
    input: &AllocationIdentityMemory,
    profile: IdentityTransferProfile,
) -> (AllocationIdentityMemory, AllocationIdentityMemory) {
    let Some(function) = rust_function_from_node(node_id) else {
        return (input.clone(), input.clone());
    };

    let mut out = input.clone();
    let mut event_summary = input.clone();
    for stmt in &bb.statements {
        // CQPL syntactic events are attached to the basic block, while identity
        // transfer is sequential.  Join the memories immediately before and
        // after every statement so an event that occurs before a later strong
        // overwrite cannot lose its AbstractAllocId at export time.
        event_summary = event_summary.join(&out);
        transfer_statement_with_profile(function, stmt, &mut out, profile);
        event_summary = event_summary.join(&out);
    }

    // Terminator events (drop/call/read/write summaries) observe the identity
    // reaching the terminator.  Allocation-return identities introduced by a
    // modeled library call are included by the post-transfer join as well.
    event_summary = event_summary.join(&out);
    if !internal_call {
        if let Some(MirTerminator::Call {
            details,
            function_called,
            arguments,
            return_place,
            ..
        }) = &bb.terminator
        {
            transfer_library_call_with_external_context(
                node_id,
                function,
                context,
                function_called,
                details,
                arguments,
                return_place,
                represented_external_body,
                arguments
                    .first()
                    .and_then(|arg| first_local_operand(function, &arg.arg))
                    .is_some_and(|source| definitely_null_at_terminator.contains(&source)),
                ffi_functions,
                &mut out,
            );
            event_summary = event_summary.join(&out);
        }
    }

    (out, event_summary)
}

fn copy_binding(
    memory: &mut AllocationIdentityMemory,
    source: &ProgramVarId,
    destination: ProgramVarId,
) {
    let allocs = memory.points_to(source);
    let refs = memory.stack_refs(source);
    let access = memory.access_bases.get(source).cloned().unwrap_or_default();
    memory.assign_points_to(destination.clone(), allocs);
    memory.assign_stack_refs(destination.clone(), refs);
    memory.assign_access_bases(destination.clone(), access);
    copy_projected_fields(memory, source, &destination);
}

/// Rebase projected fields of a closure environment object onto the closure
/// formal local.  Higher-order library summaries may activate a local closure
/// directly from its environment object rather than through the explicit
/// `&closure` temporary seen at a language-level Fn/FnMut/FnOnce callsite.
/// Copying only already-proved projected MAY relations preserves capture
/// identity without manufacturing aliases.
fn copy_projected_fields(
    memory: &mut AllocationIdentityMemory,
    source: &ProgramVarId,
    destination: &ProgramVarId,
) {
    let points: Vec<_> = memory
        .place_points_to
        .iter()
        .filter(|(place, _)| &place.base == source)
        .map(|(place, allocs)| (place.projection.clone(), allocs.clone()))
        .collect();
    let refs: Vec<_> = memory
        .place_stack_refs
        .iter()
        .filter(|(place, _)| &place.base == source)
        .map(|(place, refs)| (place.projection.clone(), refs.clone()))
        .collect();

    for (projection, allocs) in points {
        memory.assign_place_points_to(
            PlaceId { base: destination.clone(), projection },
            allocs,
        );
    }
    for (projection, refs) in refs {
        memory.assign_place_stack_refs(
            PlaceId { base: destination.clone(), projection },
            refs,
        );
    }
}

fn copy_projected_subtree(
    memory: &mut AllocationIdentityMemory,
    source: &ProgramVarId,
    source_prefix: &[PlaceProjection],
    destination: &ProgramVarId,
    destination_prefix: &[PlaceProjection],
) -> bool {
    let source_place = PlaceId {
        base: source.clone(),
        projection: source_prefix.to_vec(),
    };
    let exact_allocs = memory.points_to_place(&source_place);
    let exact_refs = memory.stack_refs_place(&source_place);

    let point_descendants: Vec<_> = memory
        .place_points_to
        .iter()
        .filter(|(place, _)| {
            place.base == *source
                && place.projection.len() >= source_prefix.len()
                && place.projection[..source_prefix.len()] == source_prefix[..]
        })
        .map(|(place, allocs)| {
            (place.projection[source_prefix.len()..].to_vec(), allocs.clone())
        })
        .collect();
    let ref_descendants: Vec<_> = memory
        .place_stack_refs
        .iter()
        .filter(|(place, _)| {
            place.base == *source
                && place.projection.len() >= source_prefix.len()
                && place.projection[..source_prefix.len()] == source_prefix[..]
        })
        .map(|(place, refs)| {
            (place.projection[source_prefix.len()..].to_vec(), refs.clone())
        })
        .collect();

    let had_projected_evidence = !exact_allocs.is_empty()
        || !exact_refs.is_empty()
        || point_descendants.iter().any(|(_, allocs)| !allocs.is_empty())
        || ref_descendants.iter().any(|(_, refs)| !refs.is_empty());

    let exact_destination = PlaceId {
        base: destination.clone(),
        projection: destination_prefix.to_vec(),
    };
    if exact_destination.projection.is_empty() {
        memory.assign_points_to(destination.clone(), exact_allocs);
        memory.assign_stack_refs(destination.clone(), exact_refs);
    } else {
        memory.assign_place_points_to(exact_destination.clone(), exact_allocs);
        memory.assign_place_stack_refs(exact_destination, exact_refs);
    }

    for (suffix, allocs) in point_descendants {
        if suffix.is_empty() {
            continue;
        }
        let mut projection = destination_prefix.to_vec();
        projection.extend(suffix);
        memory.assign_place_points_to(
            PlaceId { base: destination.clone(), projection },
            allocs,
        );
    }
    for (suffix, refs) in ref_descendants {
        if suffix.is_empty() {
            continue;
        }
        let mut projection = destination_prefix.to_vec();
        projection.extend(suffix);
        memory.assign_place_stack_refs(
            PlaceId { base: destination.clone(), projection },
            refs,
        );
    }

    had_projected_evidence
}

/// Copy a MIR downcast payload while preserving the MAY abstraction boundary.
///
/// Field-sensitive identity is preferred whenever the producer has represented
/// it.  If the wrapper binding carries only a coarse MAY identity and no
/// projected fact exists, dropping that coarse fact would turn loss of
/// precision into a false refutation.  The fallback therefore reuses only
/// identities already present on the source binding; it never creates a fresh
/// allocation or overrides represented field-sensitive evidence.
fn copy_downcast_payload(
    memory: &mut AllocationIdentityMemory,
    source: &ProgramVarId,
    source_prefix: &[PlaceProjection],
    destination: &ProgramVarId,
) {
    let fallback_allocs = memory.points_to(source);
    let fallback_refs = memory.stack_refs(source);

    clear_destination(memory, destination);
    let had_projected_evidence =
        copy_projected_subtree(memory, source, source_prefix, destination, &[]);

    if !had_projected_evidence {
        memory.assign_points_to(destination.clone(), fallback_allocs);
        memory.assign_stack_refs(destination.clone(), fallback_refs);
    }
}

fn copy_value_into_field(
    memory: &mut AllocationIdentityMemory,
    source: &ProgramVarId,
    destination: &ProgramVarId,
    field_index: usize,
) {
    copy_projected_subtree(
        memory,
        source,
        &[],
        destination,
        &[PlaceProjection::Field { index: field_index as u32 }],
    );
}

fn bind_actuals_to_formals(
    icfg: &GlobalICFGOrdered,
    call: &RustCallMetadata,
    caller_memory: &AllocationIdentityMemory,
) -> AllocationIdentityMemory {
    let mut callee_memory = caller_memory.clone();
    let Some(function) = icfg.rust_functions.get(&call.callee_function) else {
        return callee_memory;
    };

    for formal_index in 0..function.arg_count {
        let Some(argument) = call.arguments.get(formal_index) else { break; };
        let Some(actual) = first_local_operand(&call.caller_function, &argument.arg) else {
            continue;
        };
        let formal = ProgramVarId::Rust {
            function: call.callee_function.clone(),
            local: (formal_index + 1) as u32,
        };
        copy_binding(&mut callee_memory, &actual, formal.clone());
        if call.is_closure && formal_index == 0 {
            copy_projected_fields(&mut callee_memory, &actual, &formal);
        }
    }

    callee_memory
}

fn bind_return_to_caller(
    call: &RustCallMetadata,
    callee_memory: &AllocationIdentityMemory,
    caller_snapshot: &AllocationIdentityMemory,
) -> AllocationIdentityMemory {
    let mut out = caller_snapshot.join(callee_memory);
    let callee_return = ProgramVarId::Rust {
        function: call.callee_function.clone(),
        local: 0,
    };

    if let Some(caller_return) = local(&call.caller_function, &call.return_place) {
        copy_binding(&mut out, &callee_return, caller_return);
    }

    out
}

fn validate_canonical_rust_call_relation(icfg: &GlobalICFGOrdered) {
    let nodes = node_map(icfg);
    let succs = successor_map(icfg);

    let has_edge = |source: &str, destination: &str| {
        succs
            .get(source)
            .is_some_and(|nexts| nexts.contains(destination))
    };

    for call in &icfg.rust_calls {
        let Some(callee) = icfg.rust_functions.get(&call.callee_function) else {
            panic!(
                "v6K canonical ICFG invariant violated before identity analysis: missing local callee metadata '{}'",
                call.callee_function
            );
        };

        if !matches!(nodes.get(&call.call_node), Some(GlobalICFGNode::Mir(_))) {
            panic!(
                "v6K canonical ICFG invariant violated before identity analysis: call node '{}' is absent or not MIR",
                call.call_node
            );
        }

        match nodes.get(&call.dummy_call_node) {
            Some(GlobalICFGNode::DummyCall(dummy))
                if dummy.is_internal.unwrap_or(false)
                    && dummy.incoming_edge == call.call_node
                    && dummy.outgoing_edge == callee.entry_node => {}
            _ => panic!(
                "v6K canonical ICFG invariant violated before identity analysis: internal dummyCall '{}' does not encode {} -> {}",
                call.dummy_call_node, call.call_node, callee.entry_node
            ),
        }

        match nodes.get(&call.dummy_ret_node) {
            Some(GlobalICFGNode::DummyRet(dummy))
                if dummy.is_internal.unwrap_or(false)
                    && dummy.outgoing_edge == call.return_node => {}
            _ => panic!(
                "v6K canonical ICFG invariant violated before identity analysis: internal dummyRet '{}' does not return to {}",
                call.dummy_ret_node, call.return_node
            ),
        }

        if !has_edge(&call.call_node, &call.dummy_call_node) {
            panic!(
                "v6K canonical ICFG invariant violated before identity analysis: {} -> {} missing",
                call.call_node, call.dummy_call_node
            );
        }
        if !has_edge(&call.dummy_call_node, &callee.entry_node) {
            panic!(
                "v6K canonical ICFG invariant violated before identity analysis: {} -> {} missing",
                call.dummy_call_node, callee.entry_node
            );
        }
        for ret in &callee.return_nodes {
            if !has_edge(ret, &call.dummy_ret_node) {
                panic!(
                    "v6K canonical ICFG invariant violated before identity analysis: {} -> {} missing",
                    ret, call.dummy_ret_node
                );
            }
        }
        if !has_edge(&call.dummy_ret_node, &call.return_node) {
            panic!(
                "v6K canonical ICFG invariant violated before identity analysis: {} -> {} missing",
                call.dummy_ret_node, call.return_node
            );
        }
    }
}

fn join_input(
    states: &mut BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory>,
    point: IdentityAnalysisPoint,
    incoming: AllocationIdentityMemory,
) -> bool {
    match states.get(&point) {
        None => {
            states.insert(point, incoming);
            true
        }
        Some(old) => {
            let joined = old.join(&incoming);
            if &joined != old {
                states.insert(point, joined);
                true
            } else {
                false
            }
        }
    }
}

/// Run the Phase-6C identity fixed point.
///
/// This analysis is independent from the historical string/equivalence-class
/// alias representation used by the legacy detector.  It is the authoritative
/// source of `ProgramVarId -> AbstractAllocId` identity information for later
/// CQPL schema versions while legacy `CellValue` transfer functions remain
/// untouched in this slice.
pub fn fixed_point_identity_analysis(
    icfg: &GlobalICFGOrdered,
    entry: &str,
) -> AllocationIdentityState {
    fixed_point_identity_analysis_with_profile(
        icfg,
        entry,
        IdentityTransferProfile::LegacyFrozen,
    )
}

pub fn fixed_point_disposition_identity_analysis(
    icfg: &GlobalICFGOrdered,
    entry: &str,
) -> AllocationIdentityState {
    fixed_point_identity_analysis_with_profile(
        icfg,
        entry,
        IdentityTransferProfile::DispositionV6S,
    )
}

/// Opt-in R1 gate inherited by the compiler wrapper. ProgramVarId and
/// AbstractAllocId retain their existing meanings.
pub(crate) fn p3_actual_formal_enabled() -> bool {
    std::env::var("CREMA_INTERNAL_DEP1_P3_R1").as_deref() == Ok("1")
}

#[derive(Clone)]
struct ActualFormalPlan {
    activation: serde_json::Value,
    caller_scope: String,
    callee_scope: String,
    actuals: Vec<serde_json::Value>,
    formals: Vec<serde_json::Value>,
    abi: String,
}

fn actual_formal_plans(icfg: &GlobalICFGOrdered) -> Result<BTreeMap<String, ActualFormalPlan>, String> {
    let dep = icfg.dependency_body_ingestion_v1.as_ref().ok_or("p2_control_flow_incomplete: DEP1 absent")?;
    let graph = dep.context_execution_graph_v1.as_ref().ok_or("p2_control_flow_incomplete: execution graph absent")?;
    let activations = graph.get("represented_call_activations").and_then(serde_json::Value::as_array)
        .ok_or("p2_control_flow_incomplete: activation records absent")?;
    let mut plans = BTreeMap::new();
    for a in activations {
        let text = |field: &str| a.get(field).and_then(serde_json::Value::as_str).ok_or_else(|| format!("unresolved_formal: missing {field}"));
        let key = text("call_key")?;
        let callee = text("callee_instance_id")?;
        let caller = text("caller_instance_id")?;
        let static_matches: Vec<_> = dep.exact_call_bindings.iter().filter(|b| b["call_key"] == key
            && b["caller_instance_id"] == caller && b["resolved_instance_id"] == callee
            && b["body_status"] == "represented_body").collect();
        let [binding] = static_matches.as_slice() else { return Err("unresolved_actual: nonunique P2 binding".into()); };
        let bodies: Vec<_> = dep.body_statuses.iter().filter(|b| b["instance_id"] == callee && b["body_status"] == "represented_body").collect();
        let [body] = bodies.as_slice() else { return Err("unresolved_formal: nonunique represented body".into()); };
        let mut pushed = a["parent_context"].as_array().ok_or("unresolved_actual: parent context")?.clone();
        pushed.push(serde_json::Value::String(key.into()));
        if a["callee_context"] != serde_json::Value::Array(pushed) { return Err("p2_control_flow_incomplete: invalid push".into()); }
        let entry = text("callee_entry_state")?;
        let scopes: Vec<_> = icfg.rust_functions.iter().filter(|(_,f)| f.entry_node == entry).collect();
        let [(callee_scope, metadata)] = scopes.as_slice() else { return Err("unresolved_formal: entry scope".into()); };
        let caller_scope = rust_function_from_node(text("caller_call_state")?).ok_or("unresolved_actual: caller scope")?.to_string();
        let actuals = binding.get("actual_operands_v1").and_then(serde_json::Value::as_array).ok_or("unresolved_actual: compiler operand proof absent")?.clone();
        let formal_proof = &body["formal_arguments_v1"];
        let formals = formal_proof["arguments"].as_array().ok_or("unresolved_formal: compiler locals proof absent")?.clone();
        if actuals.len() != formals.len() || formals.len() != metadata.arg_count
            || formal_proof["arg_count"].as_u64() != Some(formals.len() as u64) {
            return Err("unresolved_formal: ABI/argument cardinality".into());
        }
        for (i, (actual, formal)) in actuals.iter().zip(&formals).enumerate() {
            if actual["argument_index"].as_u64() != Some(i as u64)
                || formal["argument_index"].as_u64() != Some(i as u64)
                || formal["formal_local"].as_u64() != Some((i+1) as u64) {
                return Err("unresolved_formal: compiler argument index".into());
            }
        }
        let proxy = text("dummy_call_state")?.to_string();
        if !icfg.icfg_edges.iter().any(|e| e.source == proxy && e.destination == entry) {
            return Err("p2_control_flow_incomplete: certified call-entry edge absent".into());
        }
        let plan = ActualFormalPlan { activation:a.clone(), caller_scope, callee_scope:(*callee_scope).clone(),
            actuals, formals, abi:formal_proof["abi"].as_str().unwrap_or("unsupported_callable_abi").into() };
        if plans.insert(proxy,plan).is_some() { return Err("p2_control_flow_incomplete: duplicate activation proxy".into()); }
    }
    Ok(plans)
}

/// Import only references reachable from an explicitly bound actual. This is
/// not a copy of the caller environment. Place/stack-reference edges preserve
/// their existing scoped identity; no equivalence or heap identity is invented.
fn import_referents(source: &AllocationIdentityMemory, target: &mut AllocationIdentityMemory, seeds: BTreeSet<PlaceId>) {
    let mut pending: Vec<_> = seeds.into_iter().collect();
    let mut seen = BTreeSet::new();
    while let Some(place) = pending.pop() {
        if !seen.insert(place.clone()) { continue; }
        let refs = source.stack_refs_place(&place);
        if place.projection.is_empty() {
            target.assign_points_to(place.base.clone(), source.points_to_place(&place));
            target.assign_stack_refs(place.base.clone(), refs.clone());
        } else {
            target.assign_place_points_to(place.clone(), source.points_to_place(&place));
            target.assign_place_stack_refs(place.clone(), refs.clone());
        }
        pending.extend(refs);
        if place.projection.is_empty() {
            target.assign_access_bases(place.base.clone(), source.access_bases.get(&place.base).cloned().unwrap_or_default());
            for (child, allocs) in &source.place_points_to {
                if child.base == place.base { target.assign_place_points_to(child.clone(),allocs.clone()); }
            }
            for (child, refs) in &source.place_stack_refs {
                if child.base == place.base {
                    target.assign_place_stack_refs(child.clone(),refs.clone()); pending.extend(refs.iter().cloned());
                }
            }
        }
    }
}

fn transfer_actual_formal(plan: &ActualFormalPlan, caller: &AllocationIdentityMemory)
    -> (AllocationIdentityMemory, Vec<serde_json::Value>) {
    let mut callee = AllocationIdentityMemory::default();
    let mut records = Vec::new();
    for (i,(actual,formal)) in plan.actuals.iter().zip(&plan.formals).enumerate() {
        let destination = ProgramVarId::Rust { function:plan.callee_scope.clone(),local:(i+1) as u32 };
        let source = actual["actual_local"].as_u64().map(|local| ProgramVarId::Rust {function:plan.caller_scope.clone(),local:local as u32});
        let shape = actual["value_shape"].as_str().unwrap_or("unsupported_value_shape");
        let supported = matches!(shape,"raw_pointer"|"shared_reference"|"mutable_reference"|"owned_box"|"scalar");
        let status = if plan.abi != "Rust" || !supported || shape != formal["value_shape"].as_str().unwrap_or("")
            || (actual["operand"] != "constant" && actual["direct_local"] != true)
            || (actual["operand"] == "constant" && shape != "scalar") {
            "unsupported_value_shape"
        } else if shape == "scalar" && source.as_ref().is_none_or(|s|
            caller.event_allocations(s).is_empty() && caller.stack_refs(s).is_empty()) {
            "no_tracked_resource"
        }
        else if let Some(source) = &source {
            callee.assign_points_to(destination.clone(),caller.points_to(source));
            callee.assign_access_bases(destination.clone(),caller.access_bases.get(source).cloned().unwrap_or_default());
            let refs = caller.stack_refs(source);
            callee.assign_stack_refs(destination.clone(),refs.clone());
            import_referents(caller,&mut callee,refs);
            // Direct value projections are retained only from this bound base.
            for (p,allocs) in &caller.place_points_to { if &p.base == source {
                callee.assign_place_points_to(PlaceId{base:destination.clone(),projection:p.projection.clone()},allocs.clone());
            }}
            for (p,refs) in &caller.place_stack_refs { if &p.base == source {
                callee.assign_place_stack_refs(PlaceId{base:destination.clone(),projection:p.projection.clone()},refs.clone());
                import_referents(caller,&mut callee,refs.clone());
            }}
            if caller.event_allocations(source).is_empty() && caller.stack_refs(source).is_empty() {
                "unresolved_actual"
            } else { "resource_binding_available" }
        } else { "unresolved_actual" };
        let resources = source.as_ref().filter(|_| status=="resource_binding_available")
            .map(|s|caller.event_allocations(s)).unwrap_or_default();
        records.push(serde_json::json!({
            "call_key":plan.activation["call_key"],"caller_instance_id":plan.activation["caller_instance_id"],
            "concrete_callee_instance":plan.activation["callee_instance_id"],
            "caller_execution_context":plan.activation["parent_context"],"callee_execution_context":plan.activation["callee_context"],
            "caller_call_state":plan.activation["caller_call_state"],"callee_entry_state":plan.activation["callee_entry_state"],
            "argument_index":i,"actual":source,"formal":destination,"operand":actual["operand"],
            "value_shape":shape,"resources":resources,"status":status,
            "relation_kind":"actual_to_formal","certainty":"may",
            "provenance":"rustc_call_operand_and_mir_formal_at_p2_entry_v1"
        }));
    }
    (callee,records)
}

fn fixed_point_identity_analysis_with_profile(
    icfg: &GlobalICFGOrdered,
    entry: &str,
    profile: IdentityTransferProfile,
) -> AllocationIdentityState {
    validate_canonical_rust_call_relation(icfg);
    let nodes = node_map(icfg);
    let succs = successor_map(icfg);
    let ffi_functions = load_ffi_functions("./ffi_functions.json").unwrap_or_default();
    let definitely_null = definitely_null_at_mir_terminators(icfg, entry);

    let entry_point = IdentityAnalysisPoint {
        node: entry.to_string(),
        context: Vec::new(),
    };

    let r1_enabled = p3_actual_formal_enabled();
    let r1_plans = if r1_enabled { actual_formal_plans(icfg) } else { Ok(BTreeMap::new()) };
    let mut r1_records: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut r1_entry_points = BTreeSet::new();

    let mut in_states: BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory> =
        BTreeMap::new();
    in_states.insert(entry_point.clone(), AllocationIdentityMemory::default());

    let mut post_states: BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory> =
        BTreeMap::new();
    let mut event_states: BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory> =
        BTreeMap::new();
    let mut worklist = BTreeSet::from([entry_point]);

    // Continuations are keyed by (callsite, caller context), never by worklist
    // processing order.
    let mut continuations: BTreeMap<(String, Vec<String>), IdentityContinuation> =
        BTreeMap::new();

    while let Some(point) = worklist.iter().next().cloned() {
        worklist.remove(&point);
        let Some(node) = nodes.get(&point.node) else { continue; };
        let input = in_states.get(&point).cloned().unwrap_or_default();
        let has_internal_rust_branch = has_rust_call_at(icfg, &point.node);
        let has_represented_external_body =
            has_represented_external_body_at(icfg, &point.node);

        let empty_null_facts = BTreeSet::new();
        let null_facts = definitely_null.get(&point.node).unwrap_or(&empty_null_facts);
        let (post, event_summary) = match node {
            GlobalICFGNode::Mir(bb) => transfer_mir_node(
                &point.node,
                bb,
                &point.context,
                has_internal_rust_branch,
                has_represented_external_body,
                null_facts,
                &ffi_functions,
                &input,
                profile,
            ),
            GlobalICFGNode::Llvm(llvm_node) => transfer_llvm_identity(
                &point.node,
                llvm_node,
                &point.context,
                &input,
            ),
            GlobalICFGNode::DummyCall(dummy) => {
                let post = transfer_external_dummy_call_identity(dummy, &input);
                let event_summary = input.join(&post);
                (post, event_summary)
            }
            GlobalICFGNode::DummyRet(dummy) => {
                let post = transfer_external_dummy_ret_identity(dummy, &input);
                let event_summary = input.join(&post);
                (post, event_summary)
            }
            GlobalICFGNode::Terminal(_) => (input.clone(), input.clone()),
        };

        let post_changed = post_states.get(&point) != Some(&post);
        if post_changed {
            post_states.insert(point.clone(), post.clone());
        }
        if event_states.get(&point) != Some(&event_summary) {
            event_states.insert(point.clone(), event_summary);
        }

        if let Some(plan) = r1_plans.as_ref().ok().and_then(|plans|plans.get(&point.node)) {
            if let Some(call) = rust_call_for_dummy_call(icfg, &point.node) {
                continuations.entry((call.call_node.clone(), point.context.clone())).or_default()
                    .join_snapshot(&point.context,&post);
            }
            let (callee_input,records) = transfer_actual_formal(plan,&post);
            r1_records.insert(point.node.clone(),records);
            let callee_point = IdentityAnalysisPoint {
                node:plan.activation["callee_entry_state"].as_str().expect("validated entry").into(),
                context:bounded_context(plan.activation["caller_call_state"].as_str().expect("validated call state")),
            };
            r1_entry_points.insert(callee_point.clone());
            if join_input(&mut in_states,callee_point.clone(),callee_input) { worklist.insert(callee_point); }
            continue;
        }

        if let Some(call) = rust_call_for_dummy_call(icfg, &point.node) {
            let key = (call.call_node.clone(), point.context.clone());
            continuations
                .entry(key)
                .or_default()
                .join_snapshot(&point.context, &post);

            if let Some(function) = icfg.rust_functions.get(&call.callee_function) {
                if !succs
                    .get(&point.node)
                    .is_some_and(|nexts| nexts.contains(&function.entry_node))
                {
                    panic!(
                        "v6K canonical ICFG invariant violated in identity analysis: {} -> {} missing",
                        point.node, function.entry_node
                    );
                }
                let callee_context = bounded_context(&call.call_node);
                let callee_input = if r1_enabled { AllocationIdentityMemory::default() } else { bind_actuals_to_formals(icfg, call, &post) };
                let callee_point = IdentityAnalysisPoint {
                    node: function.entry_node.clone(),
                    context: callee_context,
                };
                if join_input(&mut in_states, callee_point.clone(), callee_input) {
                    worklist.insert(callee_point);
                }
            } else {
                panic!(
                    "v6K canonical ICFG invariant violated: missing local callee metadata '{}'",
                    call.callee_function
                );
            }
            continue;
        }

        let is_return = matches!(
            node,
            GlobalICFGNode::Mir(MirBasicBlock {
                terminator: Some(MirTerminator::Return { .. }),
                ..
            })
        );

        if is_return {
            if let Some(callee) = rust_function_from_node(&point.node) {
                for call in rust_calls_for_callee(icfg, callee) {
                    // With k=1, a callee state reached from a Rust callsite is
                    // tagged by that callsite.  Root-entry functions have an
                    // empty context and therefore do not spuriously return.
                    if point.context.last().map(String::as_str)
                        != Some(call.call_node.as_str())
                    {
                        continue;
                    }

                    let matching_continuations: Vec<_> = continuations
                        .iter()
                        .filter(|((callsite, _), cont)| {
                            callsite == &call.call_node && cont.initialized
                        })
                        .map(|(_, cont)| cont.clone())
                        .collect();

                    for continuation in matching_continuations {
                        if !succs
                            .get(&point.node)
                            .is_some_and(|nexts| nexts.contains(&call.dummy_ret_node))
                        {
                            panic!(
                                "v6K canonical ICFG invariant violated in identity analysis: {} -> {} missing",
                                point.node, call.dummy_ret_node
                            );
                        }
                        let returned = bind_return_to_caller(
                            call,
                            &post,
                            &continuation.caller_memory,
                        );
                        let return_point = IdentityAnalysisPoint {
                            node: call.dummy_ret_node.clone(),
                            context: continuation.caller_context.clone(),
                        };
                        if join_input(&mut in_states, return_point.clone(), returned) {
                            worklist.insert(return_point);
                        }
                    }
                }
            }
            continue;
        }

        if let Some(nexts) = succs.get(&point.node) {
            for succ in nexts {
                let succ_point = IdentityAnalysisPoint {
                    node: succ.clone(),
                    context: point.context.clone(),
                };
                if join_input(&mut in_states, succ_point.clone(), post.clone()) {
                    worklist.insert(succ_point);
                }
            }
        }
    }

    let mut by_node: BTreeMap<String, AllocationIdentityMemory> = BTreeMap::new();
    for (point, memory) in &post_states {
        by_node
            .entry(point.node.clone())
            .and_modify(|current| *current = current.join(memory))
            .or_insert_with(|| memory.clone());
    }

    let mut event_by_node: BTreeMap<String, AllocationIdentityMemory> = BTreeMap::new();
    for (point, memory) in &event_states {
        event_by_node
            .entry(point.node.clone())
            .and_modify(|current| *current = current.join(memory))
            .or_insert_with(|| memory.clone());
    }

    let r1_report = r1_enabled.then(|| {
        let planned = r1_plans.as_ref().map(|p|p.len()).unwrap_or(0);
        let reached = r1_records.len();
        let records: Vec<_> = r1_records.into_values().flatten().collect();
        let complete = r1_plans.is_ok() && planned == reached && records.iter().all(|r| matches!(r["status"].as_str(),Some("resource_binding_available"|"no_tracked_resource")));
        let entries: Vec<_> = r1_entry_points.iter().filter_map(|p|in_states.get(p).map(|m| IdentityPointDump {
            node:p.node.clone(),context:p.context.clone(),memory:IdentityMemoryDump::from_memory(m)
        })).collect();
        serde_json::json!({"capability":"dependency_resource_identity_continuity_v1","gate":"P3-R1-AF1",
            "p3_actual_formal_complete":complete,"planned_activations":planned,"reached_activations":reached,"status":r1_plans.err(),"bindings":records,"callee_entry_states":entries,
            "semantics":"MAY_points_to_at_represented_Rust_call_entry",
            "return_continuity":"not_evaluated_R1","side_effect_continuity":"not_evaluated_R1",
            "unwind_resource_continuity":"deferred_to_P3_R3"})
    });
    AllocationIdentityState {
        dependency_resource_identity_continuity_v1:r1_report,
        by_point: post_states,
        by_node,
        event_by_point: event_states,
        event_by_node,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::{AllocationSiteId, ProgramVarId};

    fn rust(function: &str, local: u32) -> ProgramVarId {
        ProgramVarId::Rust {
            function: function.to_string(),
            local,
        }
    }

    fn alloc(label: &str) -> AbstractAllocId {
        AbstractAllocId::new(
            AllocationSiteId::Synthetic {
                scope: "test".to_string(),
                label: label.to_string(),
            },
            Vec::new(),
        )
    }

    #[test]
    fn rust_local_identity_is_function_scoped() {
        let main_3 = rust("main", 3);
        let closure_3 = rust("main::{closure#0}", 3);
        let foo_3 = rust("foo", 3);

        assert_ne!(main_3, closure_3);
        assert_ne!(main_3, foo_3);
        assert_ne!(closure_3, foo_3);

        assert_eq!(
            main_3.canonical_string(),
            "rust::main::Local(_3)"
        );
        assert_eq!(
            closure_3.canonical_string(),
            "rust::main::{closure#0}::Local(_3)"
        );
    }

    #[test]
    fn may_alias_is_not_closed_transitively() {
        let a = alloc("A");
        let b = alloc("B");
        let p = rust("main", 1);
        let q = rust("main", 2);
        let r = rust("main", 3);

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_points_to(p.clone(), BTreeSet::from([a.clone()]));
        mem.assign_points_to(q.clone(), BTreeSet::from([a, b.clone()]));
        mem.assign_points_to(r.clone(), BTreeSet::from([b]));

        assert!(mem.may_alias(&p, &q));
        assert!(mem.may_alias(&q, &r));
        assert!(!mem.may_alias(&p, &r));
    }

    #[test]
    fn join_is_pointwise_union_of_points_to_sets() {
        let p = rust("main", 1);
        let a = alloc("A");
        let b = alloc("B");

        let mut left = AllocationIdentityMemory::default();
        left.assign_fresh(p.clone(), a.clone());

        let mut right = AllocationIdentityMemory::default();
        right.assign_fresh(p.clone(), b.clone());

        let joined = left.join(&right);
        assert_eq!(joined.points_to(&p), BTreeSet::from([a, b]));
        assert!(left.leq(&joined));
        assert!(right.leq(&joined));
    }

    #[test]
    fn historical_icfg_json_without_phase6_metadata_still_deserializes() {
        let json = r#"{"ordered_nodes":[],"icfg_edges":[]}"#;
        let g: crate::structs::GlobalICFGOrdered = serde_json::from_str(json).unwrap();
        assert!(g.rust_functions.is_empty());
        assert!(g.rust_calls.is_empty());
    }

    #[test]
    fn mir_local_parser_accepts_observed_spellings() {
        assert_eq!(crate::structs::parse_mir_local_index("_3"), Some(3));
        assert_eq!(
            crate::structs::parse_mir_local_index("Local(_17) [mutable]"),
            Some(17)
        );
        assert_eq!(crate::structs::parse_mir_local_index("const 42_i32"), None);
    }

    #[test]
    fn same_singleton_abstract_id_requires_equal_singleton_may_set() {
        let p = rust("main", 1);
        let q = rust("main", 2);
        let r = rust("main", 3);
        let a = alloc("A");
        let b = alloc("B");

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(p.clone(), a.clone());
        mem.assign_fresh(q.clone(), a.clone());
        mem.assign_points_to(r.clone(), BTreeSet::from([a, b]));

        assert!(mem.same_singleton_abstract_id(&p, &q));
        assert!(!mem.same_singleton_abstract_id(&p, &r));
    }

    fn source_info() -> crate::structs::SourceInfoData {
        crate::structs::SourceInfoData {
            span: "test.rs:1:1:1:1 (#0)".to_string(),
            scope: "scope[0]".to_string(),
        }
    }

    fn call_block(
        callee: &str,
        arguments: Vec<crate::structs::MirCallArgument>,
        return_place: &str,
        return_target: &str,
    ) -> MirBasicBlock {
        MirBasicBlock {
            block_id: 0,
            statements: Vec::new(),
            terminator: Some(MirTerminator::Call {
                details: String::new(),
                source_info: String::new(),
                function_called: callee.to_string(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments,
                return_place: return_place.to_string(),
                return_target: Some(return_target.to_string()),
                unwind_target: "continue".to_string(),
            }),
        }
    }

    fn return_block(block_id: usize, statements: Vec<MirStatement>) -> MirBasicBlock {
        MirBasicBlock {
            block_id,
            statements,
            terminator: Some(MirTerminator::Return {
                details: String::new(),
                source_info: String::new(),
            }),
        }
    }

    fn copy_statement(dest: u32, source: u32) -> MirStatement {
        MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: format!("Assign((_{dest}, copy _{source}))"),
            place: Some(format!("Local(_{dest})")),
            is_mutable: Some(true),
            rvalue: Some(format!("copy _{source}")),
        }
    }

    fn edge(source: &str, destination: &str) -> crate::structs::IcfgEdge {
        crate::structs::IcfgEdge {
            source: source.to_string(),
            destination: destination.to_string(),
            label: None,
            source_label: None,
            destination_label: None,
        }
    }

    fn internal_dummy_call(
        id: &str,
        call_node: &str,
        callee_entry: &str,
    ) -> (String, GlobalICFGNode) {
        (
            id.to_string(),
            GlobalICFGNode::DummyCall(DummyNode {
                dummy_node_name: "dummyCall".to_string(),
                incoming_edge: call_node.to_string(),
                outgoing_edge: callee_entry.to_string(),
                id: format!("test::{id}"),
                mir_var: None,
                llvm_var: None,
                argument_bindings: Vec::new(),
                is_internal: Some(true),
            }),
        )
    }

    fn internal_dummy_ret(
        id: &str,
        callee: &str,
        return_node: &str,
    ) -> (String, GlobalICFGNode) {
        (
            id.to_string(),
            GlobalICFGNode::DummyRet(DummyNode {
                dummy_node_name: "dummyRet".to_string(),
                incoming_edge: callee.to_string(),
                outgoing_edge: return_node.to_string(),
                id: format!("test::{id}"),
                mir_var: None,
                llvm_var: None,
                argument_bindings: Vec::new(),
                is_internal: Some(true),
            }),
        )
    }

    #[test]
    #[should_panic(expected = "v6K canonical ICFG invariant violated before identity analysis")]
    fn phase6k_identity_rejects_metadata_without_canonical_edges() {
        use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let main_call = call_block("foo", vec![], "_1", "bb1");
        let main_ret = return_block(1, Vec::new());
        let foo_ret = return_block(0, Vec::new());
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".to_string(), GlobalICFGNode::Mir(main_call)),
                ("rust::main::bb1".to_string(), GlobalICFGNode::Mir(main_ret)),
                ("rust::foo::bb0".to_string(), GlobalICFGNode::Mir(foo_ret)),
            ],
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::from([
                (
                    "main".to_string(),
                    RustFunctionMetadata {
                        name: "main".to_string(),
                        arg_count: 0,
                        entry_node: "rust::main::bb0".to_string(),
                        return_nodes: vec!["rust::main::bb1".to_string()],
                    },
                ),
                (
                    "foo".to_string(),
                    RustFunctionMetadata {
                        name: "foo".to_string(),
                        arg_count: 0,
                        entry_node: "rust::foo::bb0".to_string(),
                        return_nodes: vec!["rust::foo::bb0".to_string()],
                    },
                ),
            ]),
            rust_calls: vec![RustCallMetadata {
                caller_function: "main".to_string(),
                call_node: "rust::main::bb0".to_string(),
                callee_function: "foo".to_string(),
                dummy_call_node: "dummyCall::main::bb0".to_string(),
                dummy_ret_node: "dummyRet::main::bb0".to_string(),
                arguments: vec![],
                return_place: "_1".to_string(),
                return_node: "rust::main::bb1".to_string(),
                is_closure: false,
            }],

            dependency_body_ingestion_v1: None,};

        let _ = fixed_point_identity_analysis(&icfg, "rust::main::bb0");
    }

    #[test]
    fn phase6c_actual_formal_and_return_preserve_allocation_id() {
        use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let main_alloc = call_block(
            "std::boxed::Box::<i32>::new",
            vec![],
            "_1",
            "bb1",
        );
        let main_call = call_block(
            "foo",
            vec![crate::structs::MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            "_2",
            "bb2",
        );
        let main_ret = return_block(2, Vec::new());
        let foo_ret = return_block(0, vec![copy_statement(0, 1)]);

        let dummy_call = "dummyCall::main::bb1";
        let dummy_ret = "dummyRet::main::bb1";
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".to_string(), GlobalICFGNode::Mir(main_alloc)),
                ("rust::main::bb1".to_string(), GlobalICFGNode::Mir(main_call)),
                internal_dummy_call(dummy_call, "rust::main::bb1", "rust::foo::bb0"),
                ("rust::foo::bb0".to_string(), GlobalICFGNode::Mir(foo_ret)),
                internal_dummy_ret(dummy_ret, "foo", "rust::main::bb2"),
                ("rust::main::bb2".to_string(), GlobalICFGNode::Mir(main_ret)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", "rust::main::bb1"),
                edge("rust::main::bb1", dummy_call),
                edge(dummy_call, "rust::foo::bb0"),
                edge("rust::foo::bb0", dummy_ret),
                edge(dummy_ret, "rust::main::bb2"),
            ],
            rust_functions: BTreeMap::from([
                (
                    "main".to_string(),
                    RustFunctionMetadata {
                        name: "main".to_string(),
                        arg_count: 0,
                        entry_node: "rust::main::bb0".to_string(),
                        return_nodes: vec!["rust::main::bb2".to_string()],
                    },
                ),
                (
                    "foo".to_string(),
                    RustFunctionMetadata {
                        name: "foo".to_string(),
                        arg_count: 1,
                        entry_node: "rust::foo::bb0".to_string(),
                        return_nodes: vec!["rust::foo::bb0".to_string()],
                    },
                ),
            ]),
            rust_calls: vec![RustCallMetadata {
                caller_function: "main".to_string(),
                call_node: "rust::main::bb1".to_string(),
                callee_function: "foo".to_string(),
                dummy_call_node: dummy_call.to_string(),
                dummy_ret_node: dummy_ret.to_string(),
                arguments: vec![crate::structs::MirCallArgument {
                    arg: "Local(_1)".to_string(),
                    is_mutable: Some(false),
                }],
                return_place: "_2".to_string(),
                return_node: "rust::main::bb2".to_string(),
                is_closure: false,
            }],

            dependency_body_ingestion_v1: None,};

        let result = fixed_point_identity_analysis(&icfg, "rust::main::bb0");
        let at_return = result.by_node.get("rust::main::bb2").unwrap();
        let main_1 = rust("main", 1);
        let main_2 = rust("main", 2);
        let foo_1 = rust("foo", 1);

        assert_eq!(at_return.points_to(&main_1), at_return.points_to(&main_2));
        assert_eq!(at_return.points_to(&main_1), at_return.points_to(&foo_1));
        assert_eq!(at_return.points_to(&main_1).len(), 1);
    }

    #[test]
    fn v6l_multi_instance_fanout_preserves_union_of_returned_allocation_ids() {
        use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        // One generic caller block has two concrete local dispatch branches.
        // Each callee creates a fresh Box allocation at a distinct allocation
        // site and returns it.  The caller continuation must conservatively
        // contain both AbstractAllocIds: choosing the first branch would lose
        // one concrete behavior and violate the MAY over-approximation.
        let main_call = call_block("<T as Trait>::new", vec![], "_1", "bb1");
        let main_ret = return_block(1, Vec::new());
        let foo_alloc = call_block("std::boxed::Box::<i32>::new", vec![], "_0", "bb1");
        let foo_ret = return_block(1, Vec::new());
        let bar_alloc = call_block("std::boxed::Box::<i32>::new", vec![], "_0", "bb1");
        let bar_ret = return_block(1, Vec::new());

        let dc0 = "dummyCall::main::bb0::instance0";
        let dr0 = "dummyRet::main::bb0::instance0";
        let dc1 = "dummyCall::main::bb0::instance1";
        let dr1 = "dummyRet::main::bb0::instance1";

        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".into(), GlobalICFGNode::Mir(main_call)),
                internal_dummy_call(dc0, "rust::main::bb0", "rust::foo::bb0"),
                ("rust::foo::bb0".into(), GlobalICFGNode::Mir(foo_alloc)),
                ("rust::foo::bb1".into(), GlobalICFGNode::Mir(foo_ret)),
                internal_dummy_ret(dr0, "foo", "rust::main::bb1"),
                internal_dummy_call(dc1, "rust::main::bb0", "rust::bar::bb0"),
                ("rust::bar::bb0".into(), GlobalICFGNode::Mir(bar_alloc)),
                ("rust::bar::bb1".into(), GlobalICFGNode::Mir(bar_ret)),
                internal_dummy_ret(dr1, "bar", "rust::main::bb1"),
                ("rust::main::bb1".into(), GlobalICFGNode::Mir(main_ret)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", dc0),
                edge(dc0, "rust::foo::bb0"),
                edge("rust::foo::bb0", "rust::foo::bb1"),
                edge("rust::foo::bb1", dr0),
                edge(dr0, "rust::main::bb1"),
                edge("rust::main::bb0", dc1),
                edge(dc1, "rust::bar::bb0"),
                edge("rust::bar::bb0", "rust::bar::bb1"),
                edge("rust::bar::bb1", dr1),
                edge(dr1, "rust::main::bb1"),
            ],
            rust_functions: BTreeMap::from([
                ("main".into(), RustFunctionMetadata {
                    name: "main".into(), arg_count: 0,
                    entry_node: "rust::main::bb0".into(),
                    return_nodes: vec!["rust::main::bb1".into()],
                }),
                ("foo".into(), RustFunctionMetadata {
                    name: "foo".into(), arg_count: 0,
                    entry_node: "rust::foo::bb0".into(),
                    return_nodes: vec!["rust::foo::bb1".into()],
                }),
                ("bar".into(), RustFunctionMetadata {
                    name: "bar".into(), arg_count: 0,
                    entry_node: "rust::bar::bb0".into(),
                    return_nodes: vec!["rust::bar::bb1".into()],
                }),
            ]),
            rust_calls: vec![
                RustCallMetadata {
                    caller_function: "main".into(), call_node: "rust::main::bb0".into(),
                    callee_function: "foo".into(), dummy_call_node: dc0.into(),
                    dummy_ret_node: dr0.into(), arguments: vec![], return_place: "_1".into(),
                    return_node: "rust::main::bb1".into(), is_closure: false,
                },
                RustCallMetadata {
                    caller_function: "main".into(), call_node: "rust::main::bb0".into(),
                    callee_function: "bar".into(), dummy_call_node: dc1.into(),
                    dummy_ret_node: dr1.into(), arguments: vec![], return_place: "_1".into(),
                    return_node: "rust::main::bb1".into(), is_closure: false,
                },
            ],

            dependency_body_ingestion_v1: None,};

        assert!(has_rust_call_at(&icfg, "rust::main::bb0"));
        assert_eq!(rust_call_for_dummy_call(&icfg, dc0).unwrap().callee_function, "foo");
        assert_eq!(rust_call_for_dummy_call(&icfg, dc1).unwrap().callee_function, "bar");

        let result = fixed_point_identity_analysis(&icfg, "rust::main::bb0");
        let continuation = result.by_node.get("rust::main::bb1").unwrap();
        let returned = continuation.points_to(&rust("main", 1));
        assert_eq!(returned.len(), 2, "both concrete Instance branches must survive the MAY join");
        let sites: BTreeSet<_> = returned.iter().map(|a| a.site.clone()).collect();
        assert!(sites.contains(&AllocationSiteId::RustCall {
            node_id: "rust::foo::bb0".into(),
            callee: "std::boxed::Box::<i32>::new".into(),
        }));
        assert!(sites.contains(&AllocationSiteId::RustCall {
            node_id: "rust::bar::bb0".into(),
            callee: "std::boxed::Box::<i32>::new".into(),
        }));
    }

    #[test]
    fn phase6c_same_site_in_two_calls_has_distinct_context_ids() {
        use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let main_entry = MirBasicBlock {
            block_id: 0,
            statements: Vec::new(),
            terminator: Some(MirTerminator::Goto {
                details: String::new(),
                source_info: String::new(),
                target: "bb1".to_string(),
            }),
        };
        let main_call_1 = call_block("foo", vec![], "_1", "bb2");
        let main_call_2 = call_block("foo", vec![], "_2", "bb3");
        let main_ret = return_block(3, Vec::new());
        let foo_alloc = call_block(
            "std::boxed::Box::<i32>::new",
            vec![],
            "_1",
            "bb1",
        );
        let foo_ret = return_block(1, vec![copy_statement(0, 1)]);

        let dummy_call_1 = "dummyCall::main::bb1";
        let dummy_ret_1 = "dummyRet::main::bb1";
        let dummy_call_2 = "dummyCall::main::bb2";
        let dummy_ret_2 = "dummyRet::main::bb2";

        let calls = vec![
            RustCallMetadata {
                caller_function: "main".to_string(),
                call_node: "rust::main::bb1".to_string(),
                callee_function: "foo".to_string(),
                dummy_call_node: dummy_call_1.to_string(),
                dummy_ret_node: dummy_ret_1.to_string(),
                arguments: vec![],
                return_place: "_1".to_string(),
                return_node: "rust::main::bb2".to_string(),
                is_closure: false,
            },
            RustCallMetadata {
                caller_function: "main".to_string(),
                call_node: "rust::main::bb2".to_string(),
                callee_function: "foo".to_string(),
                dummy_call_node: dummy_call_2.to_string(),
                dummy_ret_node: dummy_ret_2.to_string(),
                arguments: vec![],
                return_place: "_2".to_string(),
                return_node: "rust::main::bb3".to_string(),
                is_closure: false,
            },
        ];

        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".to_string(), GlobalICFGNode::Mir(main_entry)),
                ("rust::main::bb1".to_string(), GlobalICFGNode::Mir(main_call_1)),
                internal_dummy_call(dummy_call_1, "rust::main::bb1", "rust::foo::bb0"),
                ("rust::foo::bb0".to_string(), GlobalICFGNode::Mir(foo_alloc)),
                ("rust::foo::bb1".to_string(), GlobalICFGNode::Mir(foo_ret)),
                internal_dummy_ret(dummy_ret_1, "foo", "rust::main::bb2"),
                ("rust::main::bb2".to_string(), GlobalICFGNode::Mir(main_call_2)),
                internal_dummy_call(dummy_call_2, "rust::main::bb2", "rust::foo::bb0"),
                internal_dummy_ret(dummy_ret_2, "foo", "rust::main::bb3"),
                ("rust::main::bb3".to_string(), GlobalICFGNode::Mir(main_ret)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", "rust::main::bb1"),
                edge("rust::main::bb1", dummy_call_1),
                edge(dummy_call_1, "rust::foo::bb0"),
                edge("rust::foo::bb0", "rust::foo::bb1"),
                edge("rust::foo::bb1", dummy_ret_1),
                edge(dummy_ret_1, "rust::main::bb2"),
                edge("rust::main::bb2", dummy_call_2),
                edge(dummy_call_2, "rust::foo::bb0"),
                edge("rust::foo::bb1", dummy_ret_2),
                edge(dummy_ret_2, "rust::main::bb3"),
            ],
            rust_functions: BTreeMap::from([
                (
                    "main".to_string(),
                    RustFunctionMetadata {
                        name: "main".to_string(),
                        arg_count: 0,
                        entry_node: "rust::main::bb0".to_string(),
                        return_nodes: vec!["rust::main::bb3".to_string()],
                    },
                ),
                (
                    "foo".to_string(),
                    RustFunctionMetadata {
                        name: "foo".to_string(),
                        arg_count: 0,
                        entry_node: "rust::foo::bb0".to_string(),
                        return_nodes: vec!["rust::foo::bb1".to_string()],
                    },
                ),
            ]),
            rust_calls: calls,

            dependency_body_ingestion_v1: None,};

        let result = fixed_point_identity_analysis(&icfg, "rust::main::bb0");
        let foo_alloc_state = result.by_node.get("rust::foo::bb0").unwrap();
        let foo_1 = rust("foo", 1);
        let ids = foo_alloc_state.points_to(&foo_1);
        assert_eq!(ids.len(), 2);

        let contexts: BTreeSet<Vec<String>> = ids.into_iter().map(|a| a.context).collect();
        assert_eq!(
            contexts,
            BTreeSet::from([
                vec!["rust::main::bb1".to_string()],
                vec!["rust::main::bb2".to_string()],
            ])
        );
    }

    #[test]
    fn phase6g_event_summary_preserves_identity_before_later_same_block_overwrite() {
        let p = rust("main", 1);
        let replacement = rust("main", 4);
        let a = alloc("A");
        let b = alloc("B");

        let mut input = AllocationIdentityMemory::default();
        input.assign_fresh(p.clone(), a.clone());
        input.assign_fresh(replacement.clone(), b.clone());

        let read_stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_2, copy (*_1)))".to_string(),
            place: Some("Local(_2) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("copy (*_1)".to_string()),
        };
        let overwrite_stmt = copy_statement(1, 4);
        let bb = MirBasicBlock {
            block_id: 0,
            statements: vec![read_stmt, overwrite_stmt],
            terminator: None,
        };

        let (post, event_summary) = transfer_mir_node(
            "rust::main::bb0",
            &bb,
            &[],
            false,
            false,
            &BTreeSet::new(),
            &HashSet::new(),
            &input,
            IdentityTransferProfile::LegacyFrozen,
        );

        assert_eq!(post.points_to(&p), BTreeSet::from([b.clone()]));
        assert_eq!(
            event_summary.event_allocations(&p),
            BTreeSet::from([a, b])
        );
    }

    #[test]
    fn v6s_legacy_identity_keeps_frozen_projected_lhs_behavior() {
        let raw = rust("main", 9);
        let allocation = alloc("A");
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(raw.clone(), allocation);

        let stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign(((*_9), move (_14.0: i32)))".to_string(),
            place: Some("Local(_9) -> *".to_string()),
            is_mutable: Some(false),
            rvalue: Some("move (_14.0: i32)".to_string()),
        };

        transfer_statement("main", &stmt, &mut mem);

        assert!(mem.points_to(&raw).is_empty());
    }

    #[test]
    fn v6s_projected_deref_write_preserves_base_pointer_identity() {
        let raw = rust("main", 9);
        let allocation = alloc("A");
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(raw.clone(), allocation.clone());

        let stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign(((*_9), move (_14.0: i32)))".to_string(),
            place: Some("Local(_9) -> *".to_string()),
            is_mutable: Some(false),
            rvalue: Some("move (_14.0: i32)".to_string()),
        };

        transfer_statement_with_profile(
            "main",
            &stmt,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        assert_eq!(mem.points_to(&raw), BTreeSet::from([allocation]));
    }

    #[test]
    fn v6s_projected_field_write_does_not_strong_overwrite_base_identity() {
        let base = rust("main", 9);
        let allocation = alloc("A");
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(base.clone(), allocation.clone());

        let stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign(((_9.0: i32), const 1_i32))".to_string(),
            place: Some("Local(_9) -> Field(0, Type: i32)".to_string()),
            is_mutable: Some(false),
            rvalue: Some("const 1_i32".to_string()),
        };

        transfer_statement_with_profile(
            "main",
            &stmt,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        assert_eq!(mem.points_to(&base), BTreeSet::from([allocation]));
    }

    #[test]
    fn v6s_direct_local_unknown_assignment_still_strong_overwrites_identity() {
        let local = rust("main", 9);
        let allocation = alloc("A");
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(local.clone(), allocation);

        let stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_9, const 1_i32))".to_string(),
            place: Some("Local(_9)".to_string()),
            is_mutable: Some(false),
            rvalue: Some("const 1_i32".to_string()),
        };

        transfer_statement("main", &stmt, &mut mem);

        assert!(mem.points_to(&local).is_empty());
    }

    #[test]
    fn phase6c_reference_load_separates_reference_from_heap_identity() {
        let mut mem = AllocationIdentityMemory::default();
        let owner = rust("main", 1);
        let reference = rust("main", 2);
        let loaded = rust("main", 3);
        let a = alloc("A");
        mem.assign_fresh(owner.clone(), a.clone());

        let ref_stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_2, &_1))".to_string(),
            place: Some("Local(_2) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("&_1".to_string()),
        };
        transfer_statement("main", &ref_stmt, &mut mem);

        assert!(mem.points_to(&reference).is_empty());
        assert_eq!(mem.stack_refs(&reference).len(), 1);

        let load_stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_3, copy (*_2)))".to_string(),
            place: Some("Local(_3) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("copy (*_2)".to_string()),
        };
        transfer_statement("main", &load_stmt, &mut mem);

        assert_eq!(mem.points_to(&loaded), BTreeSet::from([a]));
        assert!(!mem.same_singleton_abstract_id(&reference, &loaded));
    }

    #[test]
    fn phase6d_identity_dump_is_json_serializable() {
        let mut state = AllocationIdentityState::default();
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(rust("main", 1), alloc("A"));
        state.by_node.insert("rust::main::bb0".to_string(), mem);

        let json = serde_json::to_string(&state.to_dump()).unwrap();
        assert!(json.contains("CREMA Allocation Identity v6G"));
        assert!(json.contains("rust::main::Local(_1)"));
    }

    #[test]
    fn phase6d_pointer_call_matcher_accepts_observed_method_turbofish_without_generic_cast_aliasing() {
        assert!(is_identity_preserving_pointer_call(
            "std::boxed::Box::<i32>::into_raw"
        ));
        assert!(is_identity_preserving_pointer_call(
            "std::boxed::Box::<i32>::from_raw"
        ));
        assert!(is_identity_preserving_pointer_call(
            "std::ptr::mut_ptr::<impl *mut i32>::cast::<std::ffi::c_void>"
        ));
        assert!(is_identity_preserving_pointer_call(
            "std::ffi::CStr::from_ptr::<'_>"
        ));
        assert!(is_identity_preserving_pointer_call(
            "std::vec::Vec::<u8>::from_raw_parts"
        ));
        assert!(is_identity_preserving_pointer_call(
            "std::string::String::from_raw_parts"
        ));
        assert!(!is_identity_preserving_pointer_call(
            "some::unrelated::Value::cast::<usize>"
        ));
        assert!(!is_identity_preserving_pointer_call(
            "std::boxed::Box::<i32>::new"
        ));
    }

    #[test]
    fn v6l_string_from_raw_parts_preserves_existing_allocation_identity() {
        let mut mem = AllocationIdentityMemory::default();
        let context = Vec::<String>::new();
        let source = rust("main", 1);
        let result = rust("main", 2);
        let a = alloc("C_MALLOC_A");
        mem.assign_fresh(source.clone(), a.clone());

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &context,
            "std::string::String::from_raw_parts",
            &[
                crate::structs::MirCallArgument {
                    arg: "Local(_1) [mutable]".to_string(),
                    is_mutable: Some(true),
                },
                crate::structs::MirCallArgument {
                    arg: "const 5_usize".to_string(),
                    is_mutable: None,
                },
                crate::structs::MirCallArgument {
                    arg: "const 6_usize".to_string(),
                    is_mutable: None,
                },
            ],
            "_2",
            &mut mem,
        );

        assert_eq!(mem.points_to(&result), BTreeSet::from([a]));
    }

    #[test]
    fn phase6j_cstring_new_is_fresh_only_for_observed_borrowed_str_constructor() {
        assert!(is_fresh_rust_allocator(
            "std::ffi::CString::new::<&str>"
        ));
        assert_eq!(
            memory_events::rust_allocation_semantics("std::ffi::CString::new::<Vec<u8>>"),
            RustAllocationSemantics::MayFreshOrTransfer
        );
        assert_eq!(
            memory_events::rust_allocation_semantics("std::ffi::CString::new::<String>"),
            RustAllocationSemantics::MayFreshOrTransfer
        );
    }

    #[test]
    fn raw_realloc_identity_is_old_or_fresh_without_invalidating_source_identity() {
        let mut mem = AllocationIdentityMemory::default();
        let source = rust("main", 1);
        let result = rust("main", 4);
        let old = alloc("old");
        mem.assign_fresh(source.clone(), old.clone());

        transfer_library_call(
            "rust::main::bb7",
            "main",
            &[],
            "std::alloc::realloc",
            &[crate::structs::MirCallArgument {
                arg: "Local(_1) [mutable]".to_string(),
                is_mutable: Some(true),
            }],
            "_4",
            &mut mem,
        );

        // Identity is a MAY relation.  The result may denote the old abstract
        // allocation (in-place success), a fresh one (moved success), or null
        // (represented by absence from the identity domain).  The source ID is
        // retained here; pointer validity is modeled separately by CellValue.
        assert_eq!(mem.points_to(&source), BTreeSet::from([old.clone()]));
        let result_ids = mem.points_to(&result);
        assert!(result_ids.contains(&old));
        assert_eq!(result_ids.len(), 2);
        assert!(result_ids.iter().any(|id| matches!(
            &id.site,
            AllocationSiteId::RustCall { node_id, callee }
                if node_id == "rust::main::bb7" && callee == "std::alloc::realloc"
        )));
    }

    #[test]
    fn phase6j_cstring_result_extraction_is_type_restricted() {
        assert!(is_cstring_result_extractor(
            "std::result::Result::<std::ffi::CString, std::ffi::NulError>::unwrap"
        ));
        assert!(is_cstring_result_extractor(
            "std::result::Result::<std::ffi::CString, std::ffi::NulError>::expect"
        ));
        assert!(!is_cstring_result_extractor(
            "std::result::Result::<Vec<u8>, E>::unwrap"
        ));
        assert!(!is_cstring_result_extractor(
            "std::result::Result::<&str, std::str::Utf8Error>::unwrap"
        ));
    }

    #[test]
    fn phase6j_cstring_new_unwrap_into_raw_from_raw_preserves_one_may_allocation_id() {
        let mut mem = AllocationIdentityMemory::default();
        let context = Vec::<String>::new();

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &context,
            "std::ffi::CString::new::<&str>",
            &[crate::structs::MirCallArgument {
                arg: r#"const "ciao""#.to_string(),
                is_mutable: None,
            }],
            "_2",
            &mut mem,
        );
        let result = rust("main", 2);
        let ids = mem.points_to(&result);
        assert_eq!(ids.len(), 1);

        transfer_library_call(
            "rust::main::bb1",
            "main",
            &context,
            "std::result::Result::<std::ffi::CString, std::ffi::NulError>::unwrap",
            &[crate::structs::MirCallArgument {
                arg: "Local(_2) [mutable]".to_string(),
                is_mutable: Some(true),
            }],
            "_1",
            &mut mem,
        );
        transfer_library_call(
            "rust::main::bb2",
            "main",
            &context,
            "std::ffi::CString::into_raw",
            &[crate::structs::MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            "_3",
            &mut mem,
        );
        transfer_library_call(
            "rust::main::bb3",
            "main",
            &context,
            "std::ffi::CString::from_raw",
            &[crate::structs::MirCallArgument {
                arg: "Local(_3)".to_string(),
                is_mutable: Some(false),
            }],
            "_4",
            &mut mem,
        );

        for local_id in [1_u32, 2, 3, 4] {
            assert_eq!(mem.points_to(&rust("main", local_id)), ids);
        }
    }

    #[test]
    fn phase6j_pointer_cast_with_method_turbofish_preserves_c_malloc_identity_without_alias_closure() {
        let mut mem = AllocationIdentityMemory::default();
        let source = rust("main", 1);
        let dest = rust("main", 4);
        let unrelated = rust("main", 9);
        let c_alloc = alloc("c-malloc");
        let other = alloc("other");
        mem.assign_fresh(source.clone(), c_alloc.clone());
        mem.assign_fresh(unrelated.clone(), other.clone());

        transfer_library_call(
            "rust::main::bb3",
            "main",
            &[],
            "std::ptr::mut_ptr::<impl *mut i32>::cast::<std::ffi::c_void>",
            &[crate::structs::MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            "_4",
            &mut mem,
        );

        assert_eq!(mem.points_to(&dest), BTreeSet::from([c_alloc]));
        assert_eq!(mem.points_to(&unrelated), BTreeSet::from([other]));
    }

    #[test]
    fn phase6d_closure_capture_flow_preserves_one_allocation_identity() {
        use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let main_alloc = call_block("std::boxed::Box::<i32>::new", vec![], "_1", "bb1");
        let main_into_raw = call_block(
            "std::boxed::Box::<i32>::into_raw",
            vec![crate::structs::MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            "_2",
            "bb2",
        );
        let main_call = MirBasicBlock {
            block_id: 2,
            statements: vec![
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_4, &_2))".to_string(),
                    place: Some("Local(_4) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("&_2".to_string()),
                },
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_3, {closure@test.rs:1:1: 1:2} { ptr: move _4 }))".to_string(),
                    place: Some("Local(_3)".to_string()),
                    is_mutable: Some(false),
                    rvalue: Some("{closure@test.rs:1:1: 1:2} { ptr: move _4 }".to_string()),
                },
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_6, &_3))".to_string(),
                    place: Some("Local(_6) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("&_3".to_string()),
                },
            ],
            terminator: Some(MirTerminator::Call {
                details: String::new(),
                source_info: String::new(),
                function_called: "<{closure@test.rs:1:1: 1:2} as std::ops::Fn<()>>::call".to_string(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![
                    crate::structs::MirCallArgument {
                        arg: "Local(_6) [mutable]".to_string(),
                        is_mutable: Some(true),
                    },
                    crate::structs::MirCallArgument {
                        arg: "const ()".to_string(),
                        is_mutable: None,
                    },
                ],
                return_place: "_5".to_string(),
                return_target: Some("bb3".to_string()),
                unwind_target: "continue".to_string(),
            }),
        };
        let main_ret = return_block(3, Vec::new());

        let closure_first = MirBasicBlock {
            block_id: 0,
            statements: vec![
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_8, deref_copy ((*_1).0: &*mut i32)))".to_string(),
                    place: Some("Local(_8) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("deref_copy ((*_1).0: &*mut i32)".to_string()),
                },
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_4, copy (*_8)))".to_string(),
                    place: Some("Local(_4) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("copy (*_8)".to_string()),
                },
            ],
            terminator: Some(MirTerminator::Call {
                details: String::new(),
                source_info: String::new(),
                function_called: "std::boxed::Box::<i32>::from_raw".to_string(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![crate::structs::MirCallArgument {
                    arg: "Local(_4) [mutable]".to_string(),
                    is_mutable: Some(true),
                }],
                return_place: "_3".to_string(),
                return_target: Some("bb1".to_string()),
                unwind_target: "continue".to_string(),
            }),
        };
        let closure_mid = MirBasicBlock {
            block_id: 1,
            statements: Vec::new(),
            terminator: Some(MirTerminator::Goto {
                details: String::new(),
                source_info: String::new(),
                target: "bb2".to_string(),
            }),
        };
        let closure_second = MirBasicBlock {
            block_id: 2,
            statements: vec![
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_9, deref_copy ((*_1).0: &*mut i32)))".to_string(),
                    place: Some("Local(_9) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("deref_copy ((*_1).0: &*mut i32)".to_string()),
                },
                MirStatement {
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_7, copy (*_9)))".to_string(),
                    place: Some("Local(_7) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("copy (*_9)".to_string()),
                },
            ],
            terminator: Some(MirTerminator::Call {
                details: String::new(),
                source_info: String::new(),
                function_called: "std::boxed::Box::<i32>::from_raw".to_string(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![crate::structs::MirCallArgument {
                    arg: "Local(_7) [mutable]".to_string(),
                    is_mutable: Some(true),
                }],
                return_place: "_6".to_string(),
                return_target: Some("bb3".to_string()),
                unwind_target: "continue".to_string(),
            }),
        };
        let closure_ret = return_block(3, Vec::new());

        let closure_name = "main::{closure#0}".to_string();
        let dummy_call = "dummyCall::main::bb2";
        let dummy_ret = "dummyRet::main::bb2";
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".to_string(), GlobalICFGNode::Mir(main_alloc)),
                ("rust::main::bb1".to_string(), GlobalICFGNode::Mir(main_into_raw)),
                ("rust::main::bb2".to_string(), GlobalICFGNode::Mir(main_call)),
                internal_dummy_call(dummy_call, "rust::main::bb2", "rust::main::{closure#0}::bb0"),
                ("rust::main::{closure#0}::bb0".to_string(), GlobalICFGNode::Mir(closure_first)),
                ("rust::main::{closure#0}::bb1".to_string(), GlobalICFGNode::Mir(closure_mid)),
                ("rust::main::{closure#0}::bb2".to_string(), GlobalICFGNode::Mir(closure_second)),
                ("rust::main::{closure#0}::bb3".to_string(), GlobalICFGNode::Mir(closure_ret)),
                internal_dummy_ret(dummy_ret, &closure_name, "rust::main::bb3"),
                ("rust::main::bb3".to_string(), GlobalICFGNode::Mir(main_ret)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", "rust::main::bb1"),
                edge("rust::main::bb1", "rust::main::bb2"),
                edge("rust::main::bb2", dummy_call),
                edge(dummy_call, "rust::main::{closure#0}::bb0"),
                edge("rust::main::{closure#0}::bb0", "rust::main::{closure#0}::bb1"),
                edge("rust::main::{closure#0}::bb1", "rust::main::{closure#0}::bb2"),
                edge("rust::main::{closure#0}::bb2", "rust::main::{closure#0}::bb3"),
                edge("rust::main::{closure#0}::bb3", dummy_ret),
                edge(dummy_ret, "rust::main::bb3"),
            ],
            rust_functions: BTreeMap::from([
                (
                    "main".to_string(),
                    RustFunctionMetadata {
                        name: "main".to_string(),
                        arg_count: 0,
                        entry_node: "rust::main::bb0".to_string(),
                        return_nodes: vec!["rust::main::bb3".to_string()],
                    },
                ),
                (
                    closure_name.clone(),
                    RustFunctionMetadata {
                        name: closure_name.clone(),
                        arg_count: 1,
                        entry_node: "rust::main::{closure#0}::bb0".to_string(),
                        return_nodes: vec!["rust::main::{closure#0}::bb3".to_string()],
                    },
                ),
            ]),
            rust_calls: vec![RustCallMetadata {
                caller_function: "main".to_string(),
                call_node: "rust::main::bb2".to_string(),
                callee_function: closure_name.clone(),
                dummy_call_node: dummy_call.to_string(),
                dummy_ret_node: dummy_ret.to_string(),
                arguments: vec![
                    crate::structs::MirCallArgument {
                        arg: "Local(_6) [mutable]".to_string(),
                        is_mutable: Some(true),
                    },
                    crate::structs::MirCallArgument {
                        arg: "const ()".to_string(),
                        is_mutable: None,
                    },
                ],
                return_place: "_5".to_string(),
                return_node: "rust::main::bb3".to_string(),
                is_closure: true,
            }],

            dependency_body_ingestion_v1: None,};

        let result = fixed_point_identity_analysis(&icfg, "rust::main::bb0");
        let end = result.by_node.get("rust::main::{closure#0}::bb3").unwrap();
        let first_owner = rust(&closure_name, 3);
        let second_owner = rust(&closure_name, 6);
        let first = end.points_to(&first_owner);
        let second = end.points_to(&second_owner);

        assert_eq!(first.len(), 1);
        assert_eq!(first, second);
    }


    fn svf_statement(
        stmt_type: &str,
        lhs: Option<usize>,
        rhs: Option<usize>,
        operands: Option<Vec<usize>>,
    ) -> SvfStatement {
        SvfStatement {
            stmt_id: 1,
            stmt_type: stmt_type.to_string(),
            stmt_info: String::new(),
            edge_id: None,
            pta_edge: None,
            lhs_var_id: lhs,
            rhs_var_id: rhs,
            res_var_id: None,
            operand_var_ids: operands,
            operand_vars: None,
            call_inst: None,
            is_conditional: None,
            condition_var_id: None,
            successors: None,
        }
    }

    fn llvm_node(
        node_id: usize,
        kind: &str,
        info: &str,
        statements: Vec<SvfStatement>,
    ) -> LlvmJsonNode {
        LlvmJsonNode {
            node_id,
            node_type: false,
            info: info.to_string(),
            node_kind_string: kind.to_string(),
            node_kind: 0,
            node_source_loc: String::new(),
            function_name: None,
            basic_block: None,
            basic_block_name: None,
            basic_block_info: None,
            svf_statements: statements,
            incoming_edges: Vec::new(),
            outgoing_edges: Vec::new(),
        }
    }

    #[test]
    fn bmulti_external_dummy_call_maps_each_actual_to_same_index_formal_only() {
        use crate::structs::DummyArgumentBinding;

        let mut input = AllocationIdentityMemory::default();
        let a = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let b = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "B".into() },
            Vec::new(),
        );
        input.assign_points_to(ProgramVarId::rust("main", "_1").unwrap(), [a.clone()].into());
        input.assign_points_to(ProgramVarId::rust("main", "_2").unwrap(), [b.clone()].into());

        let dummy = DummyNode {
            dummy_node_name: "dummyCall".into(),
            incoming_edge: "rust::main::bb5".into(),
            outgoing_edge: "llvm::free_second::node1::rust::main::bb5".into(),
            id: "bmulti".into(),
            mir_var: Some("_1".into()),
            llvm_var: Some("7@rust::main::bb5".into()),
            argument_bindings: vec![
                DummyArgumentBinding { arg_index: 0, mir_var: "_1".into(), llvm_var: "7@rust::main::bb5".into(), svf_may_points_to: vec![], svf_points_to_basis: None },
                DummyArgumentBinding { arg_index: 1, mir_var: "_2".into(), llvm_var: "9@rust::main::bb5".into(), svf_may_points_to: vec![], svf_points_to_basis: None },
            ],
            is_internal: Some(false),
        };

        let out = transfer_external_dummy_call_identity(&dummy, &input);
        let f0 = c_var("free_second", 7, "rust::main::bb5");
        let f1 = c_var("free_second", 9, "rust::main::bb5");
        assert_eq!(out.points_to(&f0), [a.clone()].into());
        assert_eq!(out.points_to(&f1), [b.clone()].into());
        assert!(!out.points_to(&f0).contains(&b));
        assert!(!out.points_to(&f1).contains(&a));
    }

    #[test]
    fn bmulti_same_callee_two_callsites_do_not_cross_contaminate() {
        use crate::structs::DummyArgumentBinding;

        let mut input = AllocationIdentityMemory::default();
        for (raw, label) in [("_1","A"),("_2","B"),("_3","C"),("_4","D")] {
            let id = AbstractAllocId::new(
                AllocationSiteId::Synthetic { scope: "test".into(), label: label.into() },
                Vec::new(),
            );
            input.assign_points_to(ProgramVarId::rust("main", raw).unwrap(), [id].into());
        }

        let make = |bb: usize, x: &str, y: &str| DummyNode {
            dummy_node_name: "dummyCall".into(),
            incoming_edge: format!("rust::main::bb{bb}"),
            outgoing_edge: format!("llvm::free_second::node1::rust::main::bb{bb}"),
            id: format!("call{bb}"),
            mir_var: Some(x.into()),
            llvm_var: Some(format!("7@rust::main::bb{bb}")),
            argument_bindings: vec![
                DummyArgumentBinding { arg_index: 0, mir_var: x.into(), llvm_var: format!("7@rust::main::bb{bb}"), svf_may_points_to: vec![], svf_points_to_basis: None },
                DummyArgumentBinding { arg_index: 1, mir_var: y.into(), llvm_var: format!("9@rust::main::bb{bb}"), svf_may_points_to: vec![], svf_points_to_basis: None },
            ],
            is_internal: Some(false),
        };

        let out1 = transfer_external_dummy_call_identity(&make(5, "_1", "_2"), &input);
        let out2 = transfer_external_dummy_call_identity(&make(9, "_4", "_3"), &out1);
        let b = input.points_to(&ProgramVarId::rust("main", "_2").unwrap());
        let c = input.points_to(&ProgramVarId::rust("main", "_3").unwrap());
        assert_eq!(out2.points_to(&c_var("free_second", 9, "rust::main::bb5")), b);
        assert_eq!(out2.points_to(&c_var("free_second", 9, "rust::main::bb9")), c);
        assert_ne!(
            out2.points_to(&c_var("free_second", 9, "rust::main::bb5")),
            out2.points_to(&c_var("free_second", 9, "rust::main::bb9")),
        );
    }

    #[test]
    fn phase6e_c_program_var_identity_is_callsite_scoped() {
        let a = ProgramVarId::c("cast", 36, Some("rust::main::bb1".to_string()));
        let b = ProgramVarId::c("cast", 36, Some("rust::main::bb2".to_string()));
        assert_ne!(a, b);
    }

    #[test]
    fn phase6e_rust_to_c_dummy_call_preserves_allocation_id() {
        let mut mem = AllocationIdentityMemory::default();
        let rust_ptr = rust("main", 2);
        let allocation = alloc("rust-box");
        mem.assign_fresh(rust_ptr.clone(), allocation.clone());

        let dummy = DummyNode {
            dummy_node_name: "dummyCall".to_string(),
            incoming_edge: "rust::main::bb2".to_string(),
            outgoing_edge: "llvm::cast::node10::rust::main::bb2".to_string(),
            id: "ffi-call".to_string(),
            mir_var: Some("Local(_2) [mutable]".to_string()),
            llvm_var: Some("36@rust::main::bb2".to_string()),
            argument_bindings: Vec::new(),
            is_internal: Some(false),
        };

        let out = transfer_external_dummy_call_identity(&dummy, &mem);
        let c_formal = ProgramVarId::c(
            "cast",
            36,
            Some("rust::main::bb2".to_string()),
        );
        assert_eq!(out.points_to(&c_formal), BTreeSet::from([allocation]));
    }

    #[test]
    fn phase6e_svf_phi_is_may_union_not_alias_closure() {
        let callsite = "rust::main::bb0";
        let mut mem = AllocationIdentityMemory::default();
        let a = alloc("A");
        let b = alloc("B");
        mem.assign_fresh(c_var("f", 20, callsite), a.clone());
        mem.assign_fresh(c_var("f", 30, callsite), b.clone());

        let node = llvm_node(
            12,
            "IntraBlock",
            "",
            vec![svf_statement("PhiStmt", Some(6), None, Some(vec![20, 30]))],
        );
        let (out, _event_summary) = transfer_llvm_identity(
            "llvm::f::node12::rust::main::bb0",
            &node,
            &[],
            &mem,
        );
        assert_eq!(
            out.points_to(&c_var("f", 6, callsite)),
            BTreeSet::from([a, b])
        );
    }

    #[test]
    fn phase6e_c_malloc_return_reaches_scoped_rust_return_local() {
        use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, RustFunctionMetadata};

        let main_call = call_block("c_alloc_i32", vec![], "_1", "bb1");
        let main_ret = return_block(1, Vec::new());
        let callsite = "rust::main::bb0";
        let entry_id = "llvm::c_alloc_i32::node10::rust::main::bb0";
        let alloc_id = "llvm::c_alloc_i32::node11::rust::main::bb0";
        let exit_id = "llvm::c_alloc_i32::node12::rust::main::bb0";
        let dummy_call_id = "dummyCall::rust::main::bb0::ffi";
        let dummy_ret_id = "dummyRet::rust::main::bb1::ffi";

        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".to_string(), GlobalICFGNode::Mir(main_call)),
                (
                    dummy_call_id.to_string(),
                    GlobalICFGNode::DummyCall(DummyNode {
                        dummy_node_name: "dummyCall".to_string(),
                        incoming_edge: "rust::main::bb0".to_string(),
                        outgoing_edge: entry_id.to_string(),
                        id: "dc".to_string(),
                        mir_var: None,
                        llvm_var: None,
                        argument_bindings: Vec::new(),
                        is_internal: Some(false),
                    }),
                ),
                (
                    entry_id.to_string(),
                    GlobalICFGNode::Llvm(llvm_node(10, "FunEntryBlock", "", Vec::new())),
                ),
                (
                    alloc_id.to_string(),
                    GlobalICFGNode::Llvm(llvm_node(
                        11,
                        "FunCallBlock",
                        "%call = call noalias ptr @malloc(i64 4)",
                        vec![svf_statement("AddrStmt", Some(20), None, None)],
                    )),
                ),
                (
                    exit_id.to_string(),
                    GlobalICFGNode::Llvm(llvm_node(
                        12,
                        "FunExitBlock",
                        "",
                        vec![svf_statement("PhiStmt", Some(6), None, Some(vec![20]))],
                    )),
                ),
                (
                    dummy_ret_id.to_string(),
                    GlobalICFGNode::DummyRet(DummyNode {
                        dummy_node_name: "dummyRet".to_string(),
                        incoming_edge: exit_id.to_string(),
                        outgoing_edge: "rust::main::bb1".to_string(),
                        id: "dr".to_string(),
                        mir_var: Some("_1".to_string()),
                        llvm_var: Some("6@rust::main::bb0".to_string()),
                        argument_bindings: Vec::new(),
                        is_internal: Some(false),
                    }),
                ),
                ("rust::main::bb1".to_string(), GlobalICFGNode::Mir(main_ret)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", dummy_call_id),
                edge(dummy_call_id, entry_id),
                edge(entry_id, alloc_id),
                edge(alloc_id, exit_id),
                edge(exit_id, dummy_ret_id),
                edge(dummy_ret_id, "rust::main::bb1"),
            ],
            rust_functions: BTreeMap::from([(
                "main".to_string(),
                RustFunctionMetadata {
                    name: "main".to_string(),
                    arg_count: 0,
                    entry_node: "rust::main::bb0".to_string(),
                    return_nodes: vec!["rust::main::bb1".to_string()],
                },
            )]),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};

        let result = fixed_point_identity_analysis(&icfg, "rust::main::bb0");
        let at_return = result.by_node.get("rust::main::bb1").unwrap();
        let rust_return = rust("main", 1);
        let ids = at_return.points_to(&rust_return);
        assert_eq!(ids.len(), 1);
        let id = ids.iter().next().unwrap();
        assert_eq!(
            &id.site,
            &AllocationSiteId::CCall {
                node_id: alloc_id.to_string(),
                allocator: "malloc".to_string(),
            }
        );
        assert!(at_return.points_to(&c_var("c_alloc_i32", 6, callsite)).contains(id));
    }

    #[test]
    fn v6l_higher_order_closure_binding_rebases_capture_fields() {
        let source = rust("main", 3);
        let destination = rust("main::{closure#0}", 1);
        let a = alloc("captured");
        let captured_owner = rust("main", 4);

        let source_field = PlaceId {
            base: source.clone(),
            projection: vec![PlaceProjection::Field { index: 0 }],
        };
        let destination_field = PlaceId {
            base: destination.clone(),
            projection: vec![PlaceProjection::Field { index: 0 }],
        };
        let captured_place = PlaceId {
            base: captured_owner,
            projection: Vec::new(),
        };

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_place_points_to(
            source_field.clone(),
            BTreeSet::from([a.clone()]),
        );
        mem.assign_place_stack_refs(
            source_field,
            BTreeSet::from([captured_place.clone()]),
        );

        copy_projected_fields(&mut mem, &source, &destination);

        assert_eq!(
            mem.points_to_place(&destination_field),
            BTreeSet::from([a])
        );
        assert_eq!(
            mem.stack_refs_place(&destination_field),
            BTreeSet::from([captured_place])
        );
    }

    #[test]
    fn phase6f_event_allocation_resolution_follows_stack_places_without_alias_closure() {
        let mut mem = AllocationIdentityMemory::default();
        let owner = rust("main", 1);
        let reference = rust("main::{closure#0}", 8);
        let a = alloc("A");
        mem.assign_fresh(owner.clone(), a.clone());
        mem.assign_stack_refs(
            reference.clone(),
            BTreeSet::from([PlaceId { base: owner, projection: Vec::new() }]),
        );

        assert!(mem.points_to(&reference).is_empty());
        assert_eq!(mem.event_allocations(&reference), BTreeSet::from([a]));
    }

    #[test]
    fn address_of_projected_place_is_not_classified_as_aggregate() {
        let function = "aligned_box::AlignedBox::<[T]>::realloc";
        let rvalue = "&mut ((*_1).0: std::mem::ManuallyDrop<std::boxed::Box<[T]>>)";

        assert!(aggregate_field_operands(function, rvalue).is_none());
        assert_eq!(
            direct_reference_place(function, rvalue),
            Some(PlaceId {
                base: rust(function, 1),
                projection: vec![PlaceProjection::Field { index: 0 }],
            })
        );
    }

    #[test]
    fn downcast_move_is_not_classified_as_aggregate() {
        let function = "main";
        let rvalue = "move ((_6 as Continue).0: aligned_box::AlignedBox<[T]>)";

        assert!(aggregate_field_operands(function, rvalue).is_none());
        assert_eq!(
            downcast_field_use(function, rvalue),
            Some((
                rust(function, 6),
                vec![PlaceProjection::Field { index: 0 }],
            ))
        );
    }

    #[test]
    fn downcast_payload_falls_back_to_existing_binding_may_identity() {
        let mut mem = AllocationIdentityMemory::default();
        let source = rust("main", 16);
        let destination = rust("main", 18);
        let a = alloc("cstring-result-payload");

        // Some library summaries represent the owned payload coarsely on the
        // Result/enum binding itself.  A field-sensitive downcast must not
        // erase that MAY identity merely because no projected fact exists.
        mem.assign_points_to(source.clone(), BTreeSet::from([a.clone()]));

        copy_downcast_payload(
            &mut mem,
            &source,
            &[PlaceProjection::Field { index: 0 }],
            &destination,
        );

        assert_eq!(mem.points_to(&destination), BTreeSet::from([a]));
    }

    #[test]
    fn downcast_payload_prefers_projected_identity_over_binding_fallback() {
        let mut mem = AllocationIdentityMemory::default();
        let source = rust("main", 6);
        let destination = rust("main", 7);
        let wrapper = alloc("wrapper-coarse");
        let payload = alloc("projected-payload");

        mem.assign_points_to(source.clone(), BTreeSet::from([wrapper]));
        mem.assign_place_points_to(
            PlaceId {
                base: source.clone(),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([payload.clone()]),
        );

        copy_downcast_payload(
            &mut mem,
            &source,
            &[PlaceProjection::Field { index: 0 }],
            &destination,
        );

        assert_eq!(mem.points_to(&destination), BTreeSet::from([payload]));
    }

    #[test]
    fn projected_reference_rebases_through_reference_base_to_owner_field() {
        let mut mem = AllocationIdentityMemory::default();
        let caller_owner = rust("main", 1);
        let callee_self = rust("aligned_box::AlignedBox::<[T]>::realloc", 1);
        let field_ref = rust("aligned_box::AlignedBox::<[T]>::realloc", 27);
        let a = alloc("container");

        mem.assign_place_points_to(
            PlaceId {
                base: caller_owner.clone(),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([a.clone()]),
        );
        mem.assign_stack_refs(
            callee_self,
            BTreeSet::from([PlaceId {
                base: caller_owner,
                projection: Vec::new(),
            }]),
        );

        let stmt = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_27, &mut ((*_1).0: std::mem::ManuallyDrop<std::boxed::Box<[T]>>)))".to_string(),
            place: Some("Local(_27) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("&mut ((*_1).0: std::mem::ManuallyDrop<std::boxed::Box<[T]>>)".to_string()),
        };
        transfer_statement_with_profile(
            "aligned_box::AlignedBox::<[T]>::realloc",
            &stmt,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        assert_eq!(mem.event_allocations(&field_ref), BTreeSet::from([a]));
    }

    #[test]
    fn aggregate_result_branch_unwrap_and_manually_drop_preserve_projected_owner_identity() {
        let mut mem = AllocationIdentityMemory::default();
        let a = alloc("container");
        mem.assign_fresh(rust("main", 4), a.clone());

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::mem::ManuallyDrop::<std::boxed::Box<[T]>>::new",
            &[crate::structs::MirCallArgument {
                arg: "Local(_4)".to_string(),
                is_mutable: Some(false),
            }],
            "_3",
            &mut mem,
        );

        let aligned_box = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_0, aligned_box::AlignedBox::<[T]> { container: move _3, layout: copy _2 }))".to_string(),
            place: Some("Local(_0) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("aligned_box::AlignedBox::<[T]> { container: move _3, layout: copy _2 }".to_string()),
        };
        transfer_statement_with_profile(
            "main",
            &aligned_box,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        let ok = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_5, std::result::Result::<aligned_box::AlignedBox<[T]>, E>::Ok(move _0)))".to_string(),
            place: Some("Local(_5) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("std::result::Result::<aligned_box::AlignedBox<[T]>, E>::Ok(move _0)".to_string()),
        };
        transfer_statement_with_profile(
            "main",
            &ok,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "<std::result::Result<aligned_box::AlignedBox<[T]>, E> as std::ops::Try>::branch",
            &[crate::structs::MirCallArgument {
                arg: "Local(_5) [mutable]".to_string(),
                is_mutable: Some(true),
            }],
            "_6",
            &mut mem,
        );

        let continue_value = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_7, move ((_6 as Continue).0: aligned_box::AlignedBox<[T]>)))".to_string(),
            place: Some("Local(_7)".to_string()),
            is_mutable: Some(false),
            rvalue: Some("move ((_6 as Continue).0: aligned_box::AlignedBox<[T]>)".to_string()),
        };
        transfer_statement_with_profile(
            "main",
            &continue_value,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        let ok_again = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_8, std::result::Result::<aligned_box::AlignedBox<[T]>, E>::Ok(move _7)))".to_string(),
            place: Some("Local(_8) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("std::result::Result::<aligned_box::AlignedBox<[T]>, E>::Ok(move _7)".to_string()),
        };
        transfer_statement_with_profile(
            "main",
            &ok_again,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        transfer_library_call(
            "rust::main::bb2",
            "main",
            &[],
            "std::result::Result::<aligned_box::AlignedBox<[T]>, E>::unwrap",
            &[crate::structs::MirCallArgument {
                arg: "Local(_8) [mutable]".to_string(),
                is_mutable: Some(true),
            }],
            "_9",
            &mut mem,
        );

        let ref_container = MirStatement {
            source_info: source_info(),
            kind: "Assign".to_string(),
            details: "Assign((_10, &mut (_9.0: std::mem::ManuallyDrop<std::boxed::Box<[T]>>)))".to_string(),
            place: Some("Local(_10) [mutable]".to_string()),
            is_mutable: Some(true),
            rvalue: Some("&mut (_9.0: std::mem::ManuallyDrop<std::boxed::Box<[T]>> )".to_string()),
        };
        transfer_statement_with_profile(
            "main",
            &ref_container,
            &mut mem,
            IdentityTransferProfile::DispositionV6S,
        );

        transfer_library_call(
            "rust::main::bb3",
            "main",
            &[],
            "std::mem::ManuallyDrop::<std::boxed::Box<[T]>>::take",
            &[crate::structs::MirCallArgument {
                arg: "Local(_10) [mutable]".to_string(),
                is_mutable: Some(true),
            }],
            "_11",
            &mut mem,
        );

        assert_eq!(mem.points_to(&rust("main", 11)), BTreeSet::from([a]));
    }

    #[test]
    fn phase_b_bodyless_allocator_classifier_is_exact_and_foreign_decl_gated() {
        let ffi = HashSet::from(["malloc".to_string(), "calloc".to_string(), "strdup".to_string()]);
        assert_eq!(bodyless_c_allocator_name("malloc", "", &ffi), Some("malloc"));
        assert_eq!(bodyless_c_allocator_name("calloc", "", &ffi), Some("calloc"));
        assert_eq!(bodyless_c_allocator_name("strdup", "", &ffi), Some("strdup"));
        assert_eq!(
            bodyless_c_allocator_name("b01_malloc_leak::malloc", "Call(_1 = malloc(8))", &ffi),
            Some("malloc")
        );
        assert_eq!(bodyless_c_allocator_name("libc::malloc", "", &HashSet::new()), Some("malloc"));
        assert_eq!(bodyless_c_allocator_name("libc::calloc", "", &HashSet::new()), Some("calloc"));
        assert_eq!(bodyless_c_allocator_name("libc::strdup", "", &HashSet::new()), Some("strdup"));
        assert_eq!(bodyless_c_allocator_name("malloc_wrapper", "", &ffi), None);
        assert_eq!(bodyless_c_allocator_name("my_malloc", "", &ffi), None);
        assert_eq!(bodyless_c_allocator_name("malloc", "", &HashSet::new()), None);
    }

    #[test]
    fn phase_b_bodyless_malloc_materializes_callsite_scoped_c_identity() {
        let block = call_block("malloc", vec![], "_1", "bb1");
        let ffi = HashSet::from(["malloc".to_string()]);
        let (post, events) = transfer_mir_node(
            "rust::main::bb0",
            &block,
            &[],
            false,
            false,
            &BTreeSet::new(),
            &ffi,
            &AllocationIdentityMemory::default(),
            IdentityTransferProfile::LegacyFrozen,
        );
        let expected = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "rust::main::bb0".to_string(),
                allocator: "malloc".to_string(),
            },
            Vec::new(),
        );
        assert_eq!(post.points_to(&rust("main", 1)), BTreeSet::from([expected.clone()]));
        assert_eq!(events.points_to(&rust("main", 1)), BTreeSet::from([expected]));
    }

    #[test]
    fn phase_b_bodyless_strdup_materializes_fresh_callsite_c_identity() {
        let block = call_block(
            "strdup",
            vec![crate::structs::MirCallArgument {
                arg: "Local(_9)".to_string(),
                is_mutable: Some(false),
            }],
            "_1",
            "bb1",
        );
        let ffi = HashSet::from(["strdup".to_string()]);
        let mut input = AllocationIdentityMemory::default();
        let source = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "source".into() },
            Vec::new(),
        );
        input.assign_points_to(rust("main", 9), BTreeSet::from([source.clone()]));
        let (post, events) = transfer_mir_node(
            "rust::main::bb0",
            &block,
            &[],
            false,
            false,
            &BTreeSet::new(),
            &ffi,
            &input,
            IdentityTransferProfile::LegacyFrozen,
        );
        let expected = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "rust::main::bb0".to_string(),
                allocator: "strdup".to_string(),
            },
            Vec::new(),
        );
        assert_eq!(post.points_to(&rust("main", 1)), BTreeSet::from([expected.clone()]));
        assert_eq!(events.points_to(&rust("main", 1)), BTreeSet::from([expected]));
        assert_eq!(post.points_to(&rust("main", 9)), BTreeSet::from([source]));
        assert_ne!(post.points_to(&rust("main", 1)), post.points_to(&rust("main", 9)),
            "strdup return must be fresh and must not alias the input string allocation");
    }

    #[test]
    fn phase_b_bodyless_strdup_does_not_duplicate_represented_c_body_identity() {
        let block = call_block(
            "strdup",
            vec![crate::structs::MirCallArgument {
                arg: "Local(_9)".to_string(),
                is_mutable: Some(false),
            }],
            "_1",
            "bb1",
        );
        let ffi = HashSet::from(["strdup".to_string()]);
        let (post, _) = transfer_mir_node(
            "rust::main::bb0",
            &block,
            &[],
            false,
            true,
            &BTreeSet::new(),
            &ffi,
            &AllocationIdentityMemory::default(),
            IdentityTransferProfile::LegacyFrozen,
        );
        assert!(post.points_to(&rust("main", 1)).is_empty());
    }

    #[test]
    fn phase_b_bodyless_malloc_does_not_duplicate_represented_c_body_identity() {
        let block = call_block("malloc", vec![], "_1", "bb1");
        let ffi = HashSet::from(["malloc".to_string()]);
        let (post, _) = transfer_mir_node(
            "rust::main::bb0",
            &block,
            &[],
            false,
            true,
            &BTreeSet::new(),
            &ffi,
            &AllocationIdentityMemory::default(),
            IdentityTransferProfile::LegacyFrozen,
        );
        assert!(post.points_to(&rust("main", 1)).is_empty());
    }

    #[test]
    fn rn1_null_constructor_classifier_is_canonical_and_closed() {
        assert!(is_null_pointer_constructor_def_path(Some("core::ptr::null_mut")));
        assert!(is_null_pointer_constructor_def_path(Some("std::ptr::null_mut")));
        assert!(!is_null_pointer_constructor_def_path(Some("crate::null_mut")));
        assert!(!is_null_pointer_constructor_def_path(Some("core::ptr::null")));
        assert!(!is_null_pointer_constructor_def_path(None));
    }

    #[test]
    fn rn1_definite_null_dataflow_propagates_copy_to_realloc_terminator() {
        use crate::structs::GlobalICFGOrdered;

        let mut null_call = call_block("null_mut", vec![], "_1", "bb1");
        if let Some(MirTerminator::Call { callee_def_path, .. }) = null_call.terminator.as_mut() {
            *callee_def_path = Some("core::ptr::null_mut".to_string());
        }
        let copy = copy_statement(2, 1);
        let realloc = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_2)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "const 64_usize".into(), is_mutable: None },
            ],
            "_3",
            "bb3",
        );
        let middle = MirBasicBlock {
            block_id: 1,
            statements: vec![copy],
            terminator: Some(MirTerminator::Goto {
                details: String::new(),
                source_info: String::new(),
                target: "bb2".into(),
            }),
        };
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".into(), GlobalICFGNode::Mir(null_call)),
                ("rust::main::bb1".into(), GlobalICFGNode::Mir(middle)),
                ("rust::main::bb2".into(), GlobalICFGNode::Mir(realloc)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", "rust::main::bb1"),
                edge("rust::main::bb1", "rust::main::bb2"),
            ],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let states = definitely_null_at_mir_terminators(&icfg, "rust::main::bb0");
        assert!(states["rust::main::bb2"].contains(&rust("main", 2)));
    }

    #[test]
    fn rn1_definite_null_join_requires_all_incoming_paths() {
        use crate::structs::GlobalICFGOrdered;

        let mut null_call = call_block("null_mut", vec![], "_1", "bb2");
        if let Some(MirTerminator::Call { callee_def_path, .. }) = null_call.terminator.as_mut() {
            *callee_def_path = Some("core::ptr::null_mut".to_string());
        }
        let unknown = MirBasicBlock {
            block_id: 1,
            statements: vec![MirStatement {
                source_info: source_info(),
                kind: "Assign".into(),
                details: "Assign((_1, move _9))".into(),
                place: Some("Local(_1)".into()),
                is_mutable: Some(true),
                rvalue: Some("move _9".into()),
            }],
            terminator: Some(MirTerminator::Goto {
                details: String::new(),
                source_info: String::new(),
                target: "bb2".into(),
            }),
        };
        let join = return_block(2, Vec::new());
        let entry = MirBasicBlock {
            block_id: 9,
            statements: Vec::new(),
            terminator: Some(MirTerminator::SwitchInt {
                details: String::new(),
                source_info: String::new(),
                targets: vec!["bb0".into()],
                discr: "Local(_8)".into(),
                otherwise: Some("bb1".into()),
            }),
        };
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb9".into(), GlobalICFGNode::Mir(entry)),
                ("rust::main::bb0".into(), GlobalICFGNode::Mir(null_call)),
                ("rust::main::bb1".into(), GlobalICFGNode::Mir(unknown)),
                ("rust::main::bb2".into(), GlobalICFGNode::Mir(join)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb9", "rust::main::bb0"),
                edge("rust::main::bb9", "rust::main::bb1"),
                edge("rust::main::bb0", "rust::main::bb2"),
                edge("rust::main::bb1", "rust::main::bb2"),
            ],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let states = definitely_null_at_mir_terminators(&icfg, "rust::main::bb9");
        assert!(!states["rust::main::bb2"].contains(&rust("main", 1)));
    }

    #[test]
    fn rn1_definite_null_is_killed_by_intervening_unknown_call() {
        use crate::structs::GlobalICFGOrdered;

        let mut null_call = call_block("null_mut", vec![], "_1", "bb1");
        if let Some(MirTerminator::Call { callee_def_path, .. }) = null_call.terminator.as_mut() {
            *callee_def_path = Some("core::ptr::null_mut".to_string());
        }
        let unknown_call = call_block("unknown", vec![], "_9", "bb2");
        let realloc = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_1)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "const 64_usize".into(), is_mutable: None },
            ],
            "_2",
            "bb3",
        );
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".into(), GlobalICFGNode::Mir(null_call)),
                ("rust::main::bb1".into(), GlobalICFGNode::Mir(unknown_call)),
                ("rust::main::bb2".into(), GlobalICFGNode::Mir(realloc)),
            ],
            icfg_edges: vec![
                edge("rust::main::bb0", "rust::main::bb1"),
                edge("rust::main::bb1", "rust::main::bb2"),
            ],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let states = definitely_null_at_mir_terminators(&icfg, "rust::main::bb0");
        assert!(!states["rust::main::bb2"].contains(&rust("main", 1)));
    }

    #[test]
    fn rn1_represented_external_realloc_suppresses_mir_side_fresh_identity() {
        let block = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_1)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "const 64_usize".into(), is_mutable: None },
            ],
            "_2",
            "bb1",
        );
        let ffi = HashSet::from(["realloc".to_string()]);
        let nulls = BTreeSet::from([rust("main", 1)]);
        let (post, _) = transfer_mir_node(
            "rust::main::bb0", &block, &[], false, true, &nulls, &ffi,
            &AllocationIdentityMemory::default(), IdentityTransferProfile::LegacyFrozen,
        );
        assert!(post.points_to(&rust("main", 2)).is_empty());
    }

    #[test]
    fn rn1_bodyless_realloc_definitely_null_materializes_fresh_c_identity() {
        let block = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_1)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "const 64_usize".into(), is_mutable: None },
            ],
            "_2",
            "bb1",
        );
        let ffi = HashSet::from(["realloc".to_string()]);
        let nulls = BTreeSet::from([rust("main", 1)]);
        let (post, events) = transfer_mir_node(
            "rust::main::bb0",
            &block,
            &[],
            false,
            false,
            &nulls,
            &ffi,
            &AllocationIdentityMemory::default(),
            IdentityTransferProfile::LegacyFrozen,
        );
        let expected = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "rust::main::bb0".into(),
                allocator: "realloc".into(),
            },
            Vec::new(),
        );
        assert_eq!(post.points_to(&rust("main", 2)), BTreeSet::from([expected.clone()]));
        assert_eq!(events.points_to(&rust("main", 2)), BTreeSet::from([expected]));
        assert!(post.points_to(&rust("main", 1)).is_empty(), "NULL is not an old allocation identity");
    }

    #[test]
    fn rn1_bodyless_realloc_zero_size_keeps_may_fresh_resource_identity() {
        let block = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_1)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "const 0_usize".into(), is_mutable: None },
            ],
            "_2",
            "bb1",
        );
        let ffi = HashSet::from(["realloc".to_string()]);
        let nulls = BTreeSet::from([rust("main", 1)]);
        let (post, _) = transfer_mir_node(
            "rust::main::bb0", &block, &[], false, false, &nulls, &ffi,
            &AllocationIdentityMemory::default(), IdentityTransferProfile::LegacyFrozen,
        );
        assert_eq!(post.points_to(&rust("main", 2)).len(), 1,
            "malloc(0)-equivalent realloc(NULL,0) is a MAY resource, not proven absence");
    }

    #[test]
    fn rn1_bodyless_realloc_dynamic_size_keeps_may_fresh_resource_identity() {
        let block = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_1)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "copy _9".into(), is_mutable: None },
            ],
            "_2",
            "bb1",
        );
        let ffi = HashSet::from(["realloc".to_string()]);
        let nulls = BTreeSet::from([rust("main", 1)]);
        let (post, _) = transfer_mir_node(
            "rust::main::bb0", &block, &[], false, false, &nulls, &ffi,
            &AllocationIdentityMemory::default(), IdentityTransferProfile::LegacyFrozen,
        );
        assert_eq!(post.points_to(&rust("main", 2)).len(), 1,
            "unknown size includes positive/zero malloc-equivalent outcomes and remains MAY");
    }

    #[test]
    fn rn1_bodyless_realloc_without_null_proof_does_not_invent_fresh_identity() {
        let block = call_block(
            "realloc",
            vec![
                crate::structs::MirCallArgument { arg: "Local(_1)".into(), is_mutable: Some(false) },
                crate::structs::MirCallArgument { arg: "const 64_usize".into(), is_mutable: None },
            ],
            "_2",
            "bb1",
        );
        let ffi = HashSet::from(["realloc".to_string()]);
        let (post, _) = transfer_mir_node(
            "rust::main::bb0",
            &block,
            &[],
            false,
            false,
            &BTreeSet::new(),
            &ffi,
            &AllocationIdentityMemory::default(),
            IdentityTransferProfile::LegacyFrozen,
        );
        assert!(post.points_to(&rust("main", 2)).is_empty());
    }

    #[test]
    fn phase_b_bodyless_malloc_two_callsites_have_distinct_abstract_ids() {
        let ffi = HashSet::from(["malloc".to_string()]);
        let block1 = call_block("malloc", vec![], "_1", "bb1");
        let block2 = call_block("malloc", vec![], "_2", "bb2");
        let (post1, _) = transfer_mir_node(
            "rust::main::bb0", &block1, &[], false, false, &BTreeSet::new(), &ffi,
            &AllocationIdentityMemory::default(), IdentityTransferProfile::LegacyFrozen,
        );
        let (post2, _) = transfer_mir_node(
            "rust::main::bb1", &block2, &[], false, false, &BTreeSet::new(), &ffi,
            &post1, IdentityTransferProfile::LegacyFrozen,
        );
        let a = post2.points_to(&rust("main", 1));
        let b = post2.points_to(&rust("main", 2));
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_ne!(a, b);
    }

}

#[cfg(test)]
mod p3_r1_tests {
    use super::*;
    use serde_json::json;
    use crate::structs::AllocationSiteId;

    fn var(function: &str, local: u32) -> ProgramVarId {
        ProgramVarId::Rust{function:function.into(),local}
    }
    fn alloc(label: &str) -> AbstractAllocId {
        AbstractAllocId::new(AllocationSiteId::Synthetic{scope:"caller".into(),label:label.into()},vec!["existing_context".into()])
    }
    fn plan(actuals: Vec<serde_json::Value>) -> ActualFormalPlan {
        ActualFormalPlan {
            activation:json!({"call_key":"K","caller_instance_id":"I0","callee_instance_id":"I1",
                "parent_context":["outer"],"callee_context":["outer","K"],
                "caller_call_state":"call","callee_entry_state":"entry"}),
            caller_scope:"caller".into(),callee_scope:"callee".into(),abi:"Rust".into(),
            formals:actuals.iter().enumerate().map(|(i,a)|json!({"argument_index":i,"formal_local":i+1,"value_shape":a["value_shape"]})).collect(),actuals,
        }
    }
    fn pointer(local: u32) -> serde_json::Value {
        json!({"operand":"copy","actual_local":local,"direct_local":true,"value_shape":"raw_pointer"})
    }
    #[test]
    fn p3_r1_may_set_and_scoped_formal_without_caller_environment_copy() {
        let mut caller=AllocationIdentityMemory::default();
        let resources=BTreeSet::from([alloc("a"),alloc("b")]);
        caller.assign_points_to(var("caller",1),resources.clone());
        caller.assign_fresh(var("caller",8),alloc("unrelated"));
        let (entry,records)=transfer_actual_formal(&plan(vec![pointer(1)]),&caller);
        assert_eq!(entry.points_to(&var("callee",1)),resources);
        assert!(entry.points_to(&var("caller",1)).is_empty());
        assert!(entry.points_to(&var("callee",8)).is_empty());
        assert_eq!(records[0]["status"],"resource_binding_available");
    }
    #[test]
    fn p3_r1_distinct_activations_do_not_capture_other_actual() {
        let mut caller=AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller",1),alloc("a"));caller.assign_fresh(var("caller",2),alloc("b"));
        let a=transfer_actual_formal(&plan(vec![pointer(1)]),&caller).0;
        let b=transfer_actual_formal(&plan(vec![pointer(2)]),&caller).0;
        assert_eq!(a.points_to(&var("callee",1)),BTreeSet::from([alloc("a")]));
        assert_eq!(b.points_to(&var("callee",1)),BTreeSet::from([alloc("b")]));
    }
    #[test]
    fn p3_r1_aliases_preserve_same_existing_resource_and_move() {
        let mut caller=AllocationIdentityMemory::default();caller.assign_fresh(var("caller",1),alloc("a"));
        caller.assign_fresh(var("caller",2),alloc("a"));
        let mut moved=pointer(2);moved["operand"]=json!("move");
        let (entry,_)=transfer_actual_formal(&plan(vec![pointer(1),moved]),&caller);
        assert_eq!(entry.points_to(&var("callee",1)),entry.points_to(&var("callee",2)));
        assert_eq!(entry.points_to(&var("callee",1)),BTreeSet::from([alloc("a")]));
    }
    #[test]
    fn p3_r1_scalar_constant_projection_and_unresolved_statuses() {
        let mut projection=pointer(1);projection["direct_local"]=json!(false);
        let p=plan(vec![json!({"operand":"constant","value_shape":"scalar"}),projection,pointer(3)]);
        let (entry,r)=transfer_actual_formal(&p,&AllocationIdentityMemory::default());
        assert!(entry.points_to.is_empty());
        assert_eq!(r[0]["status"],"no_tracked_resource");assert_eq!(r[1]["status"],"unsupported_value_shape");
        assert_eq!(r[2]["status"],"unresolved_actual");
    }
    #[test]
    fn p3_r1_scalar_with_existing_may_relation_is_not_silently_dropped() {
        let mut caller=AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller",1),alloc("pointer_bits"));
        let mut scalar=pointer(1);scalar["value_shape"]=json!("scalar");
        let (entry,r)=transfer_actual_formal(&plan(vec![scalar]),&caller);
        assert_eq!(entry.points_to(&var("callee",1)),caller.points_to(&var("caller",1)));
        assert_eq!(r[0]["status"],"resource_binding_available");
    }

    #[test]
    fn p3_r1_reference_import_is_finite_reachable_slice() {
        let mut caller=AllocationIdentityMemory::default();let place=PlaceId{base:var("caller",2),projection:vec![]};
        caller.assign_stack_refs(var("caller",1),BTreeSet::from([place.clone()]));
        caller.assign_fresh(var("caller",2),alloc("a"));caller.assign_fresh(var("caller",8),alloc("unrelated"));
        let mut a=pointer(1);a["value_shape"]=json!("shared_reference");
        let (entry,r)=transfer_actual_formal(&plan(vec![a]),&caller);
        assert_eq!(entry.event_allocations(&var("callee",1)),BTreeSet::from([alloc("a")]));
        assert_eq!(entry.stack_refs(&var("callee",1)),BTreeSet::from([place]));
        assert!(entry.points_to(&var("caller",8)).is_empty());assert_eq!(r[0]["status"],"resource_binding_available");
    }
}
