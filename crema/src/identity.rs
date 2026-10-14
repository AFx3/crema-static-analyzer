use crate::structs::{
    AbstractAllocId, PlaceId, PlaceProjection, ProgramVarId,
    RustAllocationDispositionEvidence, RustAtomicPtrCallEvidence,
    RustAtomicPtrCallEvidenceKind, RustPlaceRelationEvidenceKind,
};
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


/// MAY summary of one value stored somewhere inside an abstract heap
/// allocation.
///
/// Projection paths are relative to the stored value. The empty path denotes
/// the value itself; non-empty paths denote already-proved projected identity
/// such as `Option<Box<T>>::Some.0`. This is intentionally an allocation-base
/// summary rather than an offset-sensitive heap: pointer arithmetic currently
/// proves only the abstract allocation base, not a unique concrete slot.
///
/// All updates are therefore weak joins. In particular, a singleton
/// `AbstractAllocId` is not a must-alias certificate because one static
/// allocation site/context can represent several concrete dynamic objects.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HeapIdentityValue {
    #[serde(default)]
    pub points_to: BTreeMap<Vec<PlaceProjection>, BTreeSet<AbstractAllocId>>,
    #[serde(default)]
    pub stack_refs: BTreeMap<Vec<PlaceProjection>, BTreeSet<PlaceId>>,
    #[serde(default)]
    pub resources: BTreeMap<Vec<PlaceProjection>, BTreeSet<AbstractAllocId>>,
    /// Access-only provenance exists only for direct local pointer values in
    /// the current identity domain, hence it is represented at the root.
    #[serde(default)]
    pub root_access_bases: BTreeSet<AbstractAllocId>,
}

impl HeapIdentityValue {
    fn is_empty(&self) -> bool {
        self.points_to.is_empty()
            && self.stack_refs.is_empty()
            && self.resources.is_empty()
            && self.root_access_bases.is_empty()
    }

    fn join_in_place(&mut self, other: &Self) {
        for (projection, allocs) in &other.points_to {
            self.points_to
                .entry(projection.clone())
                .or_default()
                .extend(allocs.iter().cloned());
        }
        for (projection, refs) in &other.stack_refs {
            self.stack_refs
                .entry(projection.clone())
                .or_default()
                .extend(refs.iter().cloned());
        }
        for (projection, resources) in &other.resources {
            self.resources
                .entry(projection.clone())
                .or_default()
                .extend(resources.iter().cloned());
        }
        self.root_access_bases
            .extend(other.root_access_bases.iter().cloned());
    }

    fn leq(&self, other: &Self) -> bool {
        let points_to = self.points_to.iter().all(|(projection, allocs)| {
            other
                .points_to
                .get(projection)
                .is_some_and(|rhs| allocs.is_subset(rhs))
        });
        let stack_refs = self.stack_refs.iter().all(|(projection, refs)| {
            other
                .stack_refs
                .get(projection)
                .is_some_and(|rhs| refs.is_subset(rhs))
        });
        let resources = self.resources.iter().all(|(projection, values)| {
            other
                .resources
                .get(projection)
                .is_some_and(|rhs| values.is_subset(rhs))
        });
        points_to
            && stack_refs
            && resources
            && self.root_access_bases.is_subset(&other.root_access_bases)
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

    /// Authoritative resource identity for MIR Places whose denotation is a
    /// tracked canonical allocation but is not itself a pointer-valued value.
    ///
    /// P3-R1P uses this relation for typed pointee Places such as `*p`.  It is
    /// intentionally distinct from `place_points_to`: the latter describes a
    /// pointer value stored in a projected Place, whereas this map describes
    /// the resource denoted by the Place itself.  Keeping the relations
    /// separate avoids pretending that a scalar pointee value "points to" its
    /// enclosing allocation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub place_resources: BTreeMap<PlaceId, BTreeSet<AbstractAllocId>>,

    /// HT2: allocation-base heap-content identity summary.
    ///
    /// A base allocation may stand for multiple dynamic objects and multiple
    /// pointer offsets, so writes are MAY weak updates and reads join all
    /// summaries for the pointer's possible access bases.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub heap_values: BTreeMap<AbstractAllocId, HeapIdentityValue>,
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

    /// Exact authoritative resource relation for a MIR Place.  This does not
    /// fall back to pointer-value or access-provenance relations.
    pub fn resources_for_place(&self, place: &PlaceId) -> BTreeSet<AbstractAllocId> {
        self.place_resources.get(place).cloned().unwrap_or_default()
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

    /// Strong overwrite of the resource denoted by one non-empty MIR Place.
    /// Only typed/certified P3-R1P transfers should populate this relation.
    pub fn assign_place_resources(
        &mut self,
        place: PlaceId,
        resources: BTreeSet<AbstractAllocId>,
    ) {
        debug_assert!(!place.projection.is_empty());
        if resources.is_empty() {
            self.place_resources.remove(&place);
        } else {
            self.place_resources.insert(place, resources);
        }
    }

    pub fn forget_var(&mut self, var: &ProgramVarId) {
        self.access_bases.remove(var);
        self.points_to.remove(var);
        self.stack_refs.remove(var);
        self.place_points_to.retain(|place, _| &place.base != var);
        self.place_stack_refs.retain(|place, _| &place.base != var);
        self.place_resources.retain(|place, _| &place.base != var);
    }

    /// Strong overwrite of one exact projected Place subtree.
    ///
    /// Writing `x.f` invalidates identity stored in `x.f` and descendants of
    /// `x.f`, but must not invalidate sibling projections such as `x.g`.
    /// The base-local identity is intentionally untouched.
    pub fn forget_place_subtree(&mut self, root: &PlaceId) {
        debug_assert!(!root.projection.is_empty());

        let below_root = |place: &PlaceId| {
            place.base == root.base
                && place.projection.len() >= root.projection.len()
                && place.projection[..root.projection.len()] == root.projection[..]
        };

        self.place_points_to.retain(|place, _| !below_root(place));
        self.place_stack_refs.retain(|place, _| !below_root(place));
        self.place_resources.retain(|place, _| !below_root(place));
    }

    /// MAY alias is intersection of points-to sets, not equivalence closure.

    /// Snapshot exactly the identity represented by one local value and its
    /// already-proved projected descendants.
    fn value_identity(&self, source: &ProgramVarId) -> HeapIdentityValue {
        let mut value = HeapIdentityValue::default();

        let root_allocs = self.points_to(source);
        if !root_allocs.is_empty() {
            value.points_to.insert(Vec::new(), root_allocs);
        }

        let root_refs = self.stack_refs(source);
        if !root_refs.is_empty() {
            value.stack_refs.insert(Vec::new(), root_refs);
        }

        value.root_access_bases = self
            .access_bases
            .get(source)
            .cloned()
            .unwrap_or_default();

        for (place, allocs) in &self.place_points_to {
            if &place.base == source
                && !allocs.is_empty()
                && place.projection.iter().all(|p| matches!(p, PlaceProjection::Field { .. }))
            {
                value
                    .points_to
                    .entry(place.projection.clone())
                    .or_default()
                    .extend(allocs.iter().cloned());
            }
        }
        for (place, refs) in &self.place_stack_refs {
            if &place.base == source
                && !refs.is_empty()
                && place.projection.iter().all(|p| matches!(p, PlaceProjection::Field { .. }))
            {
                value
                    .stack_refs
                    .entry(place.projection.clone())
                    .or_default()
                    .extend(refs.iter().cloned());
            }
        }
        for (place, resources) in &self.place_resources {
            if &place.base == source
                && !resources.is_empty()
                && place.projection.iter().all(|p| matches!(p, PlaceProjection::Field { .. }))
            {
                value
                    .resources
                    .entry(place.projection.clone())
                    .or_default()
                    .extend(resources.iter().cloned());
            }
        }

        value
    }

    /// Weakly write one value summary to every possible abstract allocation
    /// base of a pointer. This cannot be a strong update: neither a singleton
    /// abstract allocation ID nor the absence of an explicit offset proves one
    /// unique concrete heap slot.
    fn weak_write_heap_value(
        &mut self,
        bases: &BTreeSet<AbstractAllocId>,
        value: &HeapIdentityValue,
    ) {
        if value.is_empty() {
            return;
        }
        for base in bases {
            self.heap_values
                .entry(base.clone())
                .or_default()
                .join_in_place(value);
        }
    }

    /// MAY-join heap contents reachable through every possible pointer base.
    fn read_heap_value(&self, bases: &BTreeSet<AbstractAllocId>) -> HeapIdentityValue {
        let mut value = HeapIdentityValue::default();
        for base in bases {
            if let Some(stored) = self.heap_values.get(base) {
                value.join_in_place(stored);
            }
        }
        value
    }

    /// Rebase a stored heap value onto a direct MIR-local destination.
    fn materialize_heap_value(
        &mut self,
        destination: &ProgramVarId,
        value: &HeapIdentityValue,
    ) {
        self.forget_var(destination);

        let root = Vec::<PlaceProjection>::new();
        self.assign_points_to(
            destination.clone(),
            value.points_to.get(&root).cloned().unwrap_or_default(),
        );
        self.assign_stack_refs(
            destination.clone(),
            value.stack_refs.get(&root).cloned().unwrap_or_default(),
        );
        self.assign_access_bases(destination.clone(), value.root_access_bases.clone());

        for (projection, allocs) in &value.points_to {
            if projection.is_empty() {
                continue;
            }
            self.assign_place_points_to(
                PlaceId {
                    base: destination.clone(),
                    projection: projection.clone(),
                },
                allocs.clone(),
            );
        }
        for (projection, refs) in &value.stack_refs {
            if projection.is_empty() {
                continue;
            }
            self.assign_place_stack_refs(
                PlaceId {
                    base: destination.clone(),
                    projection: projection.clone(),
                },
                refs.clone(),
            );
        }
        for (projection, resources) in &value.resources {
            if projection.is_empty() {
                continue;
            }
            self.assign_place_resources(
                PlaceId {
                    base: destination.clone(),
                    projection: projection.clone(),
                },
                resources.clone(),
            );
        }
    }

    fn heap_stack_referent_seeds(&self) -> BTreeSet<PlaceId> {
        self.heap_values
            .values()
            .flat_map(|value| value.stack_refs.values())
            .flat_map(|targets| targets.iter().cloned())
            .collect()
    }

    /// Join only the global heap component of another abstract state. This is
    /// used at interprocedural boundaries so heap side effects survive without
    /// importing unrelated callee locals into the caller.
    fn join_heap_from(&mut self, other: &Self) {
        for (base, value) in &other.heap_values {
            self.heap_values
                .entry(base.clone())
                .or_default()
                .join_in_place(value);
        }
    }

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

        for (place, resources) in &other.place_resources {
            out.place_resources
                .entry(place.clone())
                .or_default()
                .extend(resources.iter().cloned());
        }

        out.join_heap_from(other);

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

        let place_resources_leq = self.place_resources.iter().all(|(place, resources)| {
            resources.is_subset(&other.resources_for_place(place))
        });

        let access_leq = self.access_bases.iter().all(|(var, allocs)| {
            allocs.is_subset(&other.access_bases.get(var).cloned().unwrap_or_default())
        });

        let heap_leq = self.heap_values.iter().all(|(base, value)| {
            other
                .heap_values
                .get(base)
                .is_some_and(|other_value| value.leq(other_value))
        });

        points_to_leq && refs_leq && place_points_to_leq && place_refs_leq
            && place_resources_leq && access_leq && heap_leq
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

/// P3-R1P result for a query over the authoritative Place/resource domain.
///
/// `Resolved` is emitted only when every traversed projection is supported and
/// the existing MAY relations justify at least one canonical allocation.  An
/// empty relation is deliberately not promoted to success: callers that need a
/// proved non-resource case must establish that fact in a separate typed gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResourcePlaceResolution {
    Resolved(BTreeSet<AbstractAllocId>),
    UnresolvedIdentity,
    UnsupportedPlace,
}

/// Resolve canonical allocation identities for a supported MIR Place without
/// inventing heap shape.
///
/// Supported semantics:
/// - exact field projections remain exact `PlaceId` projections;
/// - dereferencing a value with `points_to` yields exactly that MAY allocation
///   set when the dereference is terminal;
/// - dereferencing a value with `stack_refs` follows only those already-proved
///   referents and may continue through exact fields/dereferences;
/// - a field after a heap dereference is unresolved because the current domain
///   does not model arbitrary heap contents; and
/// - Index/Opaque projections fail closed.
///
/// This is intentionally narrower than a heap-shape analysis and is the API a
/// later R1D retry should consume instead of collapsing `*p` to `p`.
pub(crate) fn resolve_resource_place(
    memory: &AllocationIdentityMemory,
    query: &PlaceId,
) -> ResourcePlaceResolution {
    // A producer-certified relation is already authoritative for the exact
    // Place.  This is the path used by the saved Box/Unique/NonNull witness
    // after the pointer-carrier assignment has executed.
    let certified = memory.resources_for_place(query);
    if !certified.is_empty() {
        return ResourcePlaceResolution::Resolved(certified);
    }

    let mut places = BTreeSet::from([PlaceId {
        base: query.base.clone(),
        projection: Vec::new(),
    }]);

    if query.projection.is_empty() {
        let mut resources = BTreeSet::new();
        for place in places {
            resources.extend(canonical_resources_for_place(memory, place));
        }
        return if resources.is_empty() {
            ResourcePlaceResolution::UnresolvedIdentity
        } else {
            ResourcePlaceResolution::Resolved(resources)
        };
    }

    for (position, projection) in query.projection.iter().enumerate() {
        let last = position + 1 == query.projection.len();
        match projection {
            PlaceProjection::Field { index } => {
                places = places
                    .into_iter()
                    .map(|mut place| {
                        place.projection.push(PlaceProjection::Field { index: *index });
                        place
                    })
                    .collect();
            }
            PlaceProjection::Deref => {
                let mut referents = BTreeSet::new();
                let mut terminal_resources = BTreeSet::new();
                let mut saw_supported_edge = false;

                for place in &places {
                    // If this exact typed Deref Place has already been
                    // producer-certified, it is authoritative for a terminal
                    // resource query.  For deeper projections it does not by
                    // itself justify heap contents.
                    let mut deref_place = place.clone();
                    deref_place.projection.push(PlaceProjection::Deref);
                    let explicit = memory.resources_for_place(&deref_place);

                    // `points_to` is likewise sufficient for the terminal
                    // pointee resource, but not for arbitrary fields beneath
                    // that heap object.
                    let heap = memory.points_to_place(place);

                    // Exact stack references preserve Place identity and are
                    // therefore the only current relation that may continue
                    // through later exact field/deref projections.  A logical
                    // resource relation may coexist with this edge (as in a
                    // DropGlue receiver) and must not suppress it.
                    let refs = memory.stack_refs_place(place);

                    if last {
                        if !explicit.is_empty() {
                            saw_supported_edge = true;
                            terminal_resources.extend(explicit);
                        }
                        if !heap.is_empty() {
                            saw_supported_edge = true;
                            terminal_resources.extend(heap);
                        }
                        if !refs.is_empty() {
                            saw_supported_edge = true;
                            referents.extend(refs);
                        }
                    } else if !refs.is_empty() {
                        saw_supported_edge = true;
                        referents.extend(refs);
                    } else if !explicit.is_empty() || !heap.is_empty() {
                        // We know the resource reached by this dereference but
                        // have no exact Place edge for the requested deeper
                        // projection.  Do not invent heap shape.
                        return ResourcePlaceResolution::UnresolvedIdentity;
                    }
                }

                if !saw_supported_edge {
                    return ResourcePlaceResolution::UnresolvedIdentity;
                }

                if last {
                    for referent in referents {
                        terminal_resources
                            .extend(canonical_resources_for_place(memory, referent));
                    }
                    return if terminal_resources.is_empty() {
                        ResourcePlaceResolution::UnresolvedIdentity
                    } else {
                        ResourcePlaceResolution::Resolved(terminal_resources)
                    };
                }

                if referents.is_empty() {
                    return ResourcePlaceResolution::UnresolvedIdentity;
                }
                places = referents;
            }
            PlaceProjection::Index { .. } | PlaceProjection::Opaque { .. } => {
                return ResourcePlaceResolution::UnsupportedPlace;
            }
        }
    }

    let mut resources = BTreeSet::new();
    for place in places {
        resources.extend(canonical_resources_for_place(memory, place));
    }
    if resources.is_empty() {
        ResourcePlaceResolution::UnresolvedIdentity
    } else {
        ResourcePlaceResolution::Resolved(resources)
    }
}

/// Canonical identity of one exact Place without ERR1 access provenance.
///
/// This helper follows only already-proved points-to, explicit P3-R1P resource
/// relations, and exact stack references.  It deliberately does not consult
/// `access_bases`, because an access-only alias is not a pointee-identity proof.
fn canonical_resources_for_place(
    memory: &AllocationIdentityMemory,
    start: PlaceId,
) -> BTreeSet<AbstractAllocId> {
    let mut out = BTreeSet::new();
    let mut pending = vec![start];
    let mut seen = BTreeSet::new();

    while let Some(place) = pending.pop() {
        if !seen.insert(place.clone()) {
            continue;
        }
        out.extend(memory.resources_for_place(&place));
        out.extend(memory.points_to_place(&place));
        pending.extend(memory.stack_refs_place(&place));

        // Rebase an exact projection through a previously-proved base
        // reference.  This is the same structural principle used elsewhere in
        // the identity domain and does not invent heap aliases.
        for mut base_target in memory.stack_refs(&place.base) {
            base_target.projection.extend(place.projection.iter().cloned());
            pending.push(base_target);
        }
    }
    out
}

/// Result of the context-sensitive allocation-identity analysis.
///
/// `by_point` is the context-sensitive post-state. `by_node` is its MAY join
/// over contexts and is the representation intended for later CQPL export.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AllocationIdentityState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_drop_receiver_resource_continuity_v1: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_resource_return_continuity_v1: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_resource_identity_continuity_v1: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_place_resource_relations_v1: Option<serde_json::Value>,
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
    pub dependency_drop_receiver_resource_continuity_v1: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_resource_return_continuity_v1: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_resource_identity_continuity_v1: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_place_resource_relations_v1: Option<serde_json::Value>,
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
    /// P3-R1P authoritative resource relation for typed MIR Places.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub place_resources: Vec<PlaceResourcesDump>,
    /// HT2 allocation-base heap-content summaries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub heap_values: Vec<HeapValueDump>,
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

#[derive(Debug, Clone, Serialize)]
pub struct PlaceResourcesDump {
    pub place: PlaceId,
    pub resources: Vec<AbstractAllocId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeapProjectionAllocationsDump {
    pub projection: Vec<PlaceProjection>,
    pub allocations: Vec<AbstractAllocId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeapProjectionStackRefsDump {
    pub projection: Vec<PlaceProjection>,
    pub targets: Vec<PlaceId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeapValueDump {
    pub allocation: AbstractAllocId,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub root_access_bases: Vec<AbstractAllocId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub points_to: Vec<HeapProjectionAllocationsDump>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack_refs: Vec<HeapProjectionStackRefsDump>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<HeapProjectionAllocationsDump>,
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
            place_resources: memory
                .place_resources
                .iter()
                .map(|(place, resources)| PlaceResourcesDump {
                    place: place.clone(),
                    resources: resources.iter().cloned().collect(),
                })
                .collect(),
            heap_values: memory
                .heap_values
                .iter()
                .map(|(allocation, value)| HeapValueDump {
                    allocation: allocation.clone(),
                    root_access_bases: value.root_access_bases.iter().cloned().collect(),
                    points_to: value
                        .points_to
                        .iter()
                        .map(|(projection, allocations)| HeapProjectionAllocationsDump {
                            projection: projection.clone(),
                            allocations: allocations.iter().cloned().collect(),
                        })
                        .collect(),
                    stack_refs: value
                        .stack_refs
                        .iter()
                        .map(|(projection, targets)| HeapProjectionStackRefsDump {
                            projection: projection.clone(),
                            targets: targets.iter().cloned().collect(),
                        })
                        .collect(),
                    resources: value
                        .resources
                        .iter()
                        .map(|(projection, allocations)| HeapProjectionAllocationsDump {
                            projection: projection.clone(),
                            allocations: allocations.iter().cloned().collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl AllocationIdentityState {
    pub fn to_dump(&self) -> AllocationIdentityDump {
        AllocationIdentityDump {
            dependency_drop_receiver_resource_continuity_v1: self.dependency_drop_receiver_resource_continuity_v1.clone(),
            dependency_resource_return_continuity_v1:self.dependency_resource_return_continuity_v1.clone(),
            dependency_resource_identity_continuity_v1: self.dependency_resource_identity_continuity_v1.clone(),
            dependency_place_resource_relations_v1: self.dependency_place_resource_relations_v1.clone(),
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

/// Parse only a direct-local field LHS emitted by `describe_place`, e.g.
///
///     Local(_2) [mutable] -> Field(3, Type: bool)
///
/// Dereferences, indices and opaque projections deliberately fail closed.
/// They require separate memory/place semantics and must not be collapsed
/// onto the carrier local.
fn direct_field_assignment_place(function: &str, place: &str) -> Option<PlaceId> {
    let mut parts = place.split(" -> ");

    let base_text = parts.next()?.trim();
    let base = first_local_operand(function, base_text)?;
    let mut projection = Vec::new();

    for component in parts {
        let component = component.trim();
        let rest = component.strip_prefix("Field(")?;
        let index_text = rest
            .split_once(',')
            .map(|(index, _)| index)
            .or_else(|| rest.split_once(')').map(|(index, _)| index))?
            .trim();
        let index = index_text.parse::<u32>().ok()?;
        projection.push(PlaceProjection::Field { index });
    }

    (!projection.is_empty()).then_some(PlaceId { base, projection })
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

/// Parse a projected value read through an already-proved stack reference.
///
/// rustc pretty-MIR uses both `deref_copy ((*r).f)` and ordinary
/// `copy ((*r).f)`/`move ((*r).f)` spellings.  These all denote the same
/// structural operation for allocation identity: read the exact projected
/// Place below one stack referent.  This parser is intentionally narrow:
/// only field projections after a syntactic `(*_N)` are accepted.  Index,
/// opaque, and arbitrary heap-dereference shape remain unresolved.
fn projected_deref_value_use(
    function: &str,
    rvalue: &str,
) -> Option<(ProgramVarId, Vec<PlaceProjection>)> {
    let text = rvalue.trim_start();
    if !(text.starts_with("deref_copy ")
        || text.starts_with("copy ")
        || text.starts_with("move "))
    {
        return None;
    }

    let reference = direct_deref_reference(function, text)?;
    let marker = "(*_";
    let start = text.find(marker)?;
    let close = text[start..].find(')')? + start;
    let suffix = &text[close + 1..];
    let suffix = suffix.split(':').next().unwrap_or(suffix);

    let mut projection = Vec::new();
    for component in suffix.split('.') {
        let component = component.trim();
        if component.is_empty() {
            continue;
        }
        let digits: String = component
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            break;
        }
        projection.push(PlaceProjection::Field {
            index: digits.parse().ok()?,
        });
    }

    (!projection.is_empty()).then_some((reference, projection))
}

/// Parse only an unprojected dereference value read such as `copy (*_8)`.
/// Any structural suffix after `(*_N)` is rejected here so unsupported
/// projected reads cannot silently collapse to their referent base.
fn direct_deref_value_use(function: &str, rvalue: &str) -> Option<ProgramVarId> {
    let text = rvalue.trim_start();
    if !(text.starts_with("deref_copy ")
        || text.starts_with("copy ")
        || text.starts_with("move "))
    {
        return None;
    }

    let reference = direct_deref_reference(function, text)?;
    let marker = "(*_";
    let start = text.find(marker)?;
    let close = text[start..].find(')')? + start;
    let structural_suffix = text[close + 1..]
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(')')
        .trim();

    structural_suffix.is_empty().then_some(reference)
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

/// Apply producer-certified identity flow declared by the versioned Rust
/// library-effect registry.
///
/// Scientific boundary: producer evidence is derived from rustc semantic
/// identity (DefId/type metadata) while that information is available.  When
/// the evidence declares `arg0 -> return` identity preservation, the identity
/// domain consumes that relation directly instead of re-discovering the API
/// from pretty-printed MIR text.  If the concrete MIR operand/destination is
/// not a direct local, this helper deliberately declines the transfer so the
/// existing conservative/legacy machinery remains in force.
fn apply_producer_certified_identity_relation(
    function: &str,
    arguments: &[crate::structs::MirCallArgument],
    return_place: &str,
    evidence: &RustAllocationDispositionEvidence,
    memory: &mut AllocationIdentityMemory,
) -> bool {
    use crate::library_effects_v1::ProducerIdentityRelation;

    match crate::library_effects_v1::producer_identity_relation_for_evidence(&evidence.kind) {
        ProducerIdentityRelation::PreserveArg0ToReturn => {
            let Some(argument) = arguments.first() else {
                return false;
            };
            let Some(dest) = direct_assignment_local(function, return_place) else {
                return false;
            };

            // Preserve the exact historical direct-local transfer first.  This
            // is intentionally kept byte-for-byte equivalent in meaning to R1.
            if let Some(source) = direct_call_value_local(function, &argument.arg) {
                let snapshot = memory.clone();
                clear_destination(memory, &dest);
                memory.assign_points_to(dest.clone(), snapshot.points_to(&source));
                memory.assign_stack_refs(dest.clone(), snapshot.stack_refs(&source));
                memory.assign_access_bases(
                    dest,
                    snapshot
                        .access_bases
                        .get(&source)
                        .cloned()
                        .unwrap_or_default(),
                );
                return true;
            }

            // R2: producer-certified summaries may consume an exact projected
            // value such as `copy ((*_1).field)`.  Resolve that Place only via
            // already-proved stack-reference/Place relations.  We do not fall
            // back to the carrier local: unresolved/indexed/opaque heap shape
            // fails closed and leaves the legacy transfer responsible.
            let Some((reference, projection)) =
                projected_deref_value_use(function, &argument.arg)
            else {
                return false;
            };
            let places = projected_places(memory, &reference, &projection);
            if places.is_empty() {
                return false;
            }

            let mut resources = BTreeSet::new();
            for place in places {
                resources.extend(canonical_resources_for_place(memory, place));
            }
            if resources.is_empty() {
                return false;
            }

            clear_destination(memory, &dest);
            memory.assign_points_to(dest, resources);
            true
        }
        ProducerIdentityRelation::None => false,
    }
}

/// Calls whose normal return preserves the represented concrete allocation.
///
/// This is a legacy textual fallback for historical artifacts and operations
/// not yet covered by producer-certified library-effect evidence. It is a
/// direct source-to-destination MAY flow relation, not alias closure.
fn is_std_nonnull_call(callee: &str) -> bool {
    [
        "std::ptr::NonNull::<",
        "core::ptr::NonNull::<",
        "std::ptr::non_null::NonNull::<",
        "core::ptr::non_null::NonNull::<",
    ]
    .iter()
    .any(|prefix| callee.contains(prefix))
}

fn is_std_nonnull_identity_call(callee: &str) -> bool {
    is_std_nonnull_call(callee)
        && ["new_unchecked", "as_ptr", "as_ref", "as_mut", "cast"]
            .iter()
            .any(|method| method_terminal(callee, method))
}

fn is_known_borrowed_pointer_view_call(callee: &str) -> bool {
    let known_family = callee.contains("CString")
        || callee.contains("CStr")
        || callee.contains("Vec::<")
        || callee.contains("String")
        || is_std_nonnull_call(callee)
        || callee.contains("str>::as_ptr")
        || callee.contains("slice::<impl [");
    known_family
        && (method_terminal(callee, "as_ptr") || method_terminal(callee, "as_mut_ptr"))
}

fn is_identity_preserving_pointer_call(callee: &str) -> bool {
    let typed_terminal = |family: &str, method: &str| {
        callee.contains(family) && method_terminal(callee, method)
    };
    let pointer_family = callee.contains("std::ptr::")
        || callee.contains("core::ptr::")
        || is_std_nonnull_call(callee);

    typed_terminal("Box", "into_raw")
        || typed_terminal("Box", "from_raw")
        || typed_terminal("CString", "into_raw")
        || typed_terminal("CString", "from_raw")
        || typed_terminal("CStr", "from_ptr")
        || typed_terminal("Vec", "from_raw_parts")
        || typed_terminal("String", "from_raw_parts")
        || is_std_nonnull_identity_call(callee)
        || is_known_borrowed_pointer_view_call(callee)
        || (pointer_family && method_terminal(callee, "cast"))
        || (pointer_family && method_terminal(callee, "wrapping_offset"))
        || (pointer_family && method_terminal(callee, "offset"))
        || (pointer_family && method_terminal(callee, "add"))
        || (pointer_family && method_terminal(callee, "sub"))
        || memory_events::is_into_vec_transfer_call(callee)
}


/// HT2 pointer-memory operations whose Rust semantics copy a value between a
/// raw pointer location and a MIR value without running ordinary assignment
/// transfer on the pointee.
///
/// Keep this classifier exact: similarly named wrappers are not heap-transfer
/// evidence.
fn is_ptr_read_identity_call(callee: &str) -> bool {
    callee.contains("std::ptr::read::<")
        || callee.contains("core::ptr::read::<")
        || callee.contains("std::ptr::read_unaligned::<")
        || callee.contains("core::ptr::read_unaligned::<")
        || callee.contains("std::ptr::read_volatile::<")
        || callee.contains("core::ptr::read_volatile::<")
        || callee == "std::ptr::read"
        || callee == "core::ptr::read"
}

fn is_ptr_write_identity_call(callee: &str) -> bool {
    callee.contains("std::ptr::write::<")
        || callee.contains("core::ptr::write::<")
        || callee.contains("std::ptr::write_unaligned::<")
        || callee.contains("core::ptr::write_unaligned::<")
        || callee.contains("std::ptr::write_volatile::<")
        || callee.contains("core::ptr::write_volatile::<")
        || callee == "std::ptr::write"
        || callee == "core::ptr::write"
}

/// Call arguments accepted by HT2 must denote one direct MIR local.  Reject
/// rendered dereferences/projections rather than collapsing them to the base
/// carrier via `first_local_operand`.
fn direct_call_value_local(function: &str, argument: &str) -> Option<ProgramVarId> {
    let mut text = argument.trim();
    if let Some(rest) = text.strip_prefix("move ") {
        text = rest.trim();
    } else if let Some(rest) = text.strip_prefix("copy ") {
        text = rest.trim();
    }

    if let Some((head, _)) = text.split_once(" [") {
        text = head.trim();
    }

    let direct = if let Some(inner) = text
        .strip_prefix("Local(_")
        .and_then(|tail| tail.strip_suffix(')'))
    {
        !inner.is_empty() && inner.bytes().all(|byte| byte.is_ascii_digit())
    } else if let Some(inner) = text.strip_prefix('_') {
        !inner.is_empty() && inner.bytes().all(|byte| byte.is_ascii_digit())
    } else {
        false
    };

    if direct {
        local(function, text)
    } else {
        None
    }
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

    // P3-R1P consumes only structured producer evidence and therefore runs
    // before the frozen legacy parser touches rendered Place/Rvalue text.
    if p3_place_relation_enabled() && apply_certified_place_relation(function, stmt, memory) {
        return;
    }

    let Some(place) = stmt.place.as_deref() else { return; };

    // R2B: projection-sensitive strong update for direct aggregate fields.
    //
    // A non-resource write to `x.field` changes exactly that field subtree.
    // It must not be collapsed to `x`, because doing so would erase identity
    // already proved for sibling fields.  This path is deliberately restricted
    // to direct Field projections and to RHSs with no local operand.
    // Resource-carrying projected writes and writes through dereferences remain
    // outside this rule until their value/storage semantics are represented.
    if let Some(projected_dest) = direct_field_assignment_place(function, place) {
        let local_source = stmt
            .rvalue
            .as_deref()
            .and_then(|rvalue| first_local_operand(function, rvalue));
        if local_source.is_none() {
            memory.forget_place_subtree(&projected_dest);
            return;
        }
    }

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

    if let Some((reference, projection)) = projected_deref_value_use(function, rvalue) {
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

    if let Some(reference) = direct_deref_value_use(function, rvalue) {
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

    // A rendered dereference that was not accepted by either exact parser
    // above is unsupported identity shape.  Fail closed instead of falling
    // through to `first_local_operand`, which would copy the reference/pointer
    // carrier and fabricate value identity for the dereferenced result.
    if rvalue.contains("(*_") {
        clear_destination(memory, &dest);
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
    // HT2: `ptr::write` weakly joins the identity of the written value into
    // every abstract allocation base reachable through the destination
    // pointer.  The update is intentionally weak even for a singleton base:
    // one AbstractAllocId may represent multiple dynamic objects and the
    // current pointer abstraction does not prove one unique offset/slot.
    if is_ptr_write_identity_call(callee) {
        let pointer = arguments
            .first()
            .and_then(|arg| direct_call_value_local(function, &arg.arg));
        let source = arguments
            .get(1)
            .and_then(|arg| direct_call_value_local(function, &arg.arg));

        let bases = pointer
            .as_ref()
            .map(|pointer| memory.event_allocations(pointer))
            .unwrap_or_default();
        let value = source
            .as_ref()
            .map(|source| memory.value_identity(source))
            .unwrap_or_default();

        // Unit-return bookkeeping must never collapse a projected return Place
        // onto its base local.  The heap side effect itself is independent of
        // the unit destination.
        if let Some(dest) = direct_assignment_local(function, return_place) {
            clear_destination(memory, &dest);
        }
        memory.weak_write_heap_value(&bases, &value);
        return;
    }

    // HT2: `ptr::read` is non-destructive.  Join all MAY heap summaries for
    // the pointer's possible abstract bases and rebase the stored value onto
    // one direct local destination.  A projected/opaque destination fails
    // closed instead of overwriting its base local.
    if is_ptr_read_identity_call(callee) {
        let Some(dest) = direct_assignment_local(function, return_place) else {
            return;
        };
        let pointer = arguments
            .first()
            .and_then(|arg| direct_call_value_local(function, &arg.arg));
        let bases = pointer
            .as_ref()
            .map(|pointer| memory.event_allocations(pointer))
            .unwrap_or_default();
        let value = memory.read_heap_value(&bases);

        memory.materialize_heap_value(&dest, &value);
        return;
    }

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
        RustAllocationSemantics::MayFresh => {
            // MAY-fresh constructors (notably String::from(&str)) may allocate
            // owned storage but never take ownership of the input allocation.
            // Materialize only the fresh call-site alternative: copying source
            // identity here would conflate a borrowed input with the owned
            // result and can fabricate allocator mismatches/double frees.
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
            // NonNull is a transparent non-owning pointer wrapper.  Its
            // constructors/views preserve the pointee resource even when the
            // receiver is reached through an exact stack reference (for
            // example `&mut self.ptr`).  Follow only canonical resource
            // relations; do not turn ERR1 access provenance into pointee
            // identity.
            let nonnull = is_std_nonnull_identity_call(callee);
            let allocs = if nonnull {
                tracked_resources_of_value(memory, &source)
            } else {
                memory.points_to(&source)
            };
            let refs = if nonnull {
                BTreeSet::new()
            } else {
                memory.stack_refs(&source)
            };
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


const AP1_ATOMIC_PTR_VALUE_SLOT: &str = "$crema::atomic_ptr_value_v1";

fn ap1_atomic_ptr_value_slot(mut place: PlaceId) -> PlaceId {
    place.projection.push(PlaceProjection::Opaque {
        text: AP1_ATOMIC_PTR_VALUE_SLOT.to_string(),
    });
    place
}

/// Resolve the exact MIR Places that may denote one `AtomicPtr<T>` receiver.
///
/// Methods taking `&self` normally arrive through a local reference temporary;
/// in that case only the already-proved stack-reference relation is followed.
/// A direct-local fallback is retained for by-value receivers such as
/// `AtomicPtr::into_inner` and for MIR shapes where the atomic value itself is
/// passed directly.  Heap points-to facts are deliberately *not* followed:
/// they describe the pointer value carried by a local, not the `AtomicPtr`
/// container whose hidden stored pointer AP1 models.
fn ap1_atomic_ptr_receiver_places(
    memory: &AllocationIdentityMemory,
    receiver: &ProgramVarId,
) -> BTreeSet<PlaceId> {
    let refs = memory.stack_refs(receiver);
    if !refs.is_empty() {
        return refs;
    }
    BTreeSet::from([PlaceId {
        base: receiver.clone(),
        projection: Vec::new(),
    }])
}

fn ap1_atomic_ptr_read_stored(
    memory: &AllocationIdentityMemory,
    receiver_places: &BTreeSet<PlaceId>,
) -> BTreeSet<AbstractAllocId> {
    let mut stored = BTreeSet::new();
    for place in receiver_places {
        stored.extend(memory.points_to_place(&ap1_atomic_ptr_value_slot(place.clone())));
    }
    stored
}

/// MAY update of the hidden pointer stored in an `AtomicPtr<T>`.
///
/// Except for construction of a fresh destination, AP1 uses a weak update.
/// This is required by the existing allocation-site/context abstraction:
/// one represented Place may denote multiple dynamic atomic objects, and the
/// analysis does not prove an exclusive concrete instance across threads.
/// A weak update therefore preserves every previously possible stored pointer
/// while adding the new pointer permitted by the atomic operation.
fn ap1_atomic_ptr_weak_write_stored(
    memory: &mut AllocationIdentityMemory,
    receiver_places: &BTreeSet<PlaceId>,
    new_values: &BTreeSet<AbstractAllocId>,
) {
    if new_values.is_empty() {
        return;
    }
    for place in receiver_places {
        let slot = ap1_atomic_ptr_value_slot(place.clone());
        let mut joined = memory.points_to_place(&slot);
        joined.extend(new_values.iter().cloned());
        memory.assign_place_points_to(slot, joined);
    }
}

fn ap1_direct_return_local(function: &str, return_place: &str) -> Option<ProgramVarId> {
    direct_assignment_local(function, return_place)
}

/// AP1 target-independent `AtomicPtr<T>` identity summary.
///
/// The producer in `icfg.rs` classifies calls from rustc semantic identity
/// (associated item + exact `core::sync::atomic::AtomicPtr` owner).  This
/// consumer never reconstructs semantics from pretty-printed call text.
///
/// Official Rust contracts represented here:
/// * `new(p)` initializes the atomic with `p`;
/// * `load` returns the current pointer without modifying the atomic;
/// * `store` replaces the current pointer;
/// * `swap` stores a new pointer and returns the previous pointer;
/// * `compare_exchange` and `_weak` return the previous pointer in either
///   Result variant, and write `new` only on success;
/// * deprecated `compare_and_swap` returns the previous pointer and writes
///   `new` conditionally; and
/// * `into_inner` consumes the atomic and returns its contained pointer.
///
/// The identity domain is MAY-only.  Conditional/concurrent mutations are
/// therefore weak updates.  AP1 introduces no allocation identity and no
/// deallocation fact; it only transports already-proved resource identities
/// through the AtomicPtr storage boundary.
fn apply_atomic_ptr_identity_summary(
    function: &str,
    arguments: &[crate::structs::MirCallArgument],
    return_place: &str,
    evidence: &RustAtomicPtrCallEvidence,
    memory: &mut AllocationIdentityMemory,
) -> bool {
    if evidence.owner_def_path != "core::sync::atomic::AtomicPtr"
        || evidence.callee_def_path.trim().is_empty()
    {
        return false;
    }

    let arg_local = |index: usize| {
        arguments
            .get(index)
            .and_then(|arg| first_local_operand(function, &arg.arg))
    };
    let return_local = || ap1_direct_return_local(function, return_place);

    match &evidence.kind {
        RustAtomicPtrCallEvidenceKind::New => {
            let Some(source) = arg_local(0) else { return false; };
            let Some(destination) = return_local() else { return false; };
            let stored = tracked_resources_of_value(memory, &source);

            // `new` constructs a fresh AtomicPtr value.  The wrapper itself is
            // not an alias of `p`; only its semantic hidden storage contains p.
            clear_destination(memory, &destination);
            memory.assign_place_points_to(
                ap1_atomic_ptr_value_slot(PlaceId {
                    base: destination,
                    projection: Vec::new(),
                }),
                stored,
            );
            true
        }

        RustAtomicPtrCallEvidenceKind::Load => {
            let Some(receiver) = arg_local(0) else { return false; };
            let Some(destination) = return_local() else { return false; };
            let places = ap1_atomic_ptr_receiver_places(memory, &receiver);
            let stored = ap1_atomic_ptr_read_stored(memory, &places);
            clear_destination(memory, &destination);
            memory.assign_points_to(destination, stored);
            true
        }

        RustAtomicPtrCallEvidenceKind::Store => {
            let Some(receiver) = arg_local(0) else { return false; };
            let Some(new_value) = arg_local(1) else { return false; };
            let places = ap1_atomic_ptr_receiver_places(memory, &receiver);
            let new_values = tracked_resources_of_value(memory, &new_value);
            ap1_atomic_ptr_weak_write_stored(memory, &places, &new_values);
            if let Some(destination) = return_local() {
                clear_destination(memory, &destination);
            }
            true
        }

        RustAtomicPtrCallEvidenceKind::Swap => {
            let Some(receiver) = arg_local(0) else { return false; };
            let Some(new_value) = arg_local(1) else { return false; };
            let Some(destination) = return_local() else { return false; };
            let places = ap1_atomic_ptr_receiver_places(memory, &receiver);
            let previous = ap1_atomic_ptr_read_stored(memory, &places);
            let new_values = tracked_resources_of_value(memory, &new_value);
            ap1_atomic_ptr_weak_write_stored(memory, &places, &new_values);
            clear_destination(memory, &destination);
            memory.assign_points_to(destination, previous);
            true
        }

        RustAtomicPtrCallEvidenceKind::CompareExchange
        | RustAtomicPtrCallEvidenceKind::CompareExchangeWeak => {
            let Some(receiver) = arg_local(0) else { return false; };
            let Some(expected) = arg_local(1) else { return false; };
            let Some(new_value) = arg_local(2) else { return false; };
            let Some(destination) = return_local() else { return false; };

            let places = ap1_atomic_ptr_receiver_places(memory, &receiver);
            let mut previous = ap1_atomic_ptr_read_stored(memory, &places);

            // The public contract guarantees that a successful CAS returns the
            // previous value and that on success it equals `current`.  Add the
            // already-proved expected-pointer identity as a success candidate
            // even when receiver storage could not be resolved through another
            // wrapper (e.g. an opaque carrier).  This is a MAY fact, not proof
            // that the CAS succeeds.
            previous.extend(tracked_resources_of_value(memory, &expected));

            let new_values = tracked_resources_of_value(memory, &new_value);
            ap1_atomic_ptr_weak_write_stored(memory, &places, &new_values);

            // Result<*mut T, *mut T> is represented coarsely at the wrapper
            // root. Existing downcast handling preserves this MAY identity for
            // either Ok(old) or Err(old) without inventing variant certainty.
            clear_destination(memory, &destination);
            memory.assign_points_to(destination, previous);
            true
        }

        RustAtomicPtrCallEvidenceKind::CompareAndSwap => {
            let Some(receiver) = arg_local(0) else { return false; };
            let Some(expected) = arg_local(1) else { return false; };
            let Some(new_value) = arg_local(2) else { return false; };
            let Some(destination) = return_local() else { return false; };

            let places = ap1_atomic_ptr_receiver_places(memory, &receiver);
            let mut previous = ap1_atomic_ptr_read_stored(memory, &places);
            previous.extend(tracked_resources_of_value(memory, &expected));
            let new_values = tracked_resources_of_value(memory, &new_value);
            ap1_atomic_ptr_weak_write_stored(memory, &places, &new_values);
            clear_destination(memory, &destination);
            memory.assign_points_to(destination, previous);
            true
        }

        RustAtomicPtrCallEvidenceKind::IntoInner => {
            let Some(receiver) = arg_local(0) else { return false; };
            let Some(destination) = return_local() else { return false; };
            let places = ap1_atomic_ptr_receiver_places(memory, &receiver);
            let stored = ap1_atomic_ptr_read_stored(memory, &places);
            clear_destination(memory, &destination);
            memory.assign_points_to(destination, stored);
            true
        }
    }
}


#[cfg(test)]
mod ap1_atomic_ptr_tests {
    use super::*;
    use crate::structs::{AllocationSiteId, MirCallArgument};

    fn alloc(node: &str) -> AbstractAllocId {
        AbstractAllocId::new(
            AllocationSiteId::RustCall {
                node_id: node.to_string(),
                callee: "std::boxed::Box::<u8>::new".to_string(),
            },
            vec![],
        )
    }

    fn arg(local: u32) -> MirCallArgument {
        MirCallArgument {
            arg: format!("Local(_{local})"),
            is_mutable: None,
        }
    }

    fn evidence(kind: RustAtomicPtrCallEvidenceKind, method: &str) -> RustAtomicPtrCallEvidence {
        RustAtomicPtrCallEvidence {
            kind,
            callee_def_path: format!("core::sync::atomic::AtomicPtr::{method}"),
            owner_def_path: "core::sync::atomic::AtomicPtr".to_string(),
        }
    }

    fn var(local_index: u32) -> ProgramVarId {
        ProgramVarId::Rust {
            function: "main".to_string(),
            local: local_index,
        }
    }

    fn direct_slot(local_index: u32) -> PlaceId {
        ap1_atomic_ptr_value_slot(PlaceId {
            base: var(local_index),
            projection: Vec::new(),
        })
    }

    #[test]
    fn ap1_atomic_ptr_new_and_load_preserve_contained_pointer_without_wrapper_alias() {
        let a = alloc("rust::main::bb0");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(var(1), BTreeSet::from([a.clone()]));

        assert!(apply_atomic_ptr_identity_summary(
            "main",
            &[arg(1)],
            "Local(_2)",
            &evidence(RustAtomicPtrCallEvidenceKind::New, "new"),
            &mut memory,
        ));
        assert!(memory.points_to(&var(2)).is_empty(), "AtomicPtr wrapper must not alias its payload");
        assert_eq!(memory.points_to_place(&direct_slot(2)), BTreeSet::from([a.clone()]));

        memory.assign_stack_refs(
            var(3),
            BTreeSet::from([PlaceId { base: var(2), projection: Vec::new() }]),
        );
        assert!(apply_atomic_ptr_identity_summary(
            "main",
            &[arg(3), arg(9)],
            "Local(_4)",
            &evidence(RustAtomicPtrCallEvidenceKind::Load, "load"),
            &mut memory,
        ));
        assert_eq!(memory.points_to(&var(4)), BTreeSet::from([a]));
        assert_eq!(memory.points_to_place(&direct_slot(2)).len(), 1, "load must not mutate storage");
    }

    #[test]
    fn ap1_atomic_ptr_store_is_weak_may_update() {
        let a = alloc("rust::main::bb0");
        let b = alloc("rust::main::bb1");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_place_points_to(direct_slot(2), BTreeSet::from([a.clone()]));
        memory.assign_stack_refs(
            var(3),
            BTreeSet::from([PlaceId { base: var(2), projection: Vec::new() }]),
        );
        memory.assign_points_to(var(5), BTreeSet::from([b.clone()]));

        assert!(apply_atomic_ptr_identity_summary(
            "main",
            &[arg(3), arg(5), arg(9)],
            "Local(_0)",
            &evidence(RustAtomicPtrCallEvidenceKind::Store, "store"),
            &mut memory,
        ));
        assert_eq!(memory.points_to_place(&direct_slot(2)), BTreeSet::from([a, b]));
    }

    #[test]
    fn ap1_atomic_ptr_compare_exchange_returns_previous_candidate_and_may_store_new() {
        let old = alloc("rust::main::bb0");
        let new = alloc("rust::main::bb1");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_place_points_to(direct_slot(2), BTreeSet::from([old.clone()]));
        memory.assign_stack_refs(
            var(3),
            BTreeSet::from([PlaceId { base: var(2), projection: Vec::new() }]),
        );
        memory.assign_points_to(var(4), BTreeSet::from([old.clone()]));
        memory.assign_points_to(var(5), BTreeSet::from([new.clone()]));

        assert!(apply_atomic_ptr_identity_summary(
            "main",
            &[arg(3), arg(4), arg(5), arg(9), arg(10)],
            "Local(_6)",
            &evidence(RustAtomicPtrCallEvidenceKind::CompareExchange, "compare_exchange"),
            &mut memory,
        ));
        assert_eq!(memory.points_to(&var(6)), BTreeSet::from([old.clone()]));
        assert_eq!(memory.points_to_place(&direct_slot(2)), BTreeSet::from([old, new]));
    }

    #[test]
    fn ap1_atomic_ptr_compare_exchange_unresolved_receiver_preserves_success_expected_candidate() {
        let expected = alloc("rust::main::bb0");
        let new = alloc("rust::main::bb1");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(var(4), BTreeSet::from([expected.clone()]));
        memory.assign_points_to(var(5), BTreeSet::from([new]));

        // _3 has no proven referent/storage relation.  AP1 must not pretend to
        // resolve the carrier, but the API contract still permits a successful
        // CAS whose previous value equals the already-proved `current` value.
        assert!(apply_atomic_ptr_identity_summary(
            "main",
            &[arg(3), arg(4), arg(5), arg(9), arg(10)],
            "Local(_6)",
            &evidence(RustAtomicPtrCallEvidenceKind::CompareExchange, "compare_exchange"),
            &mut memory,
        ));
        assert!(memory.points_to(&var(6)).contains(&expected));
    }

    #[test]
    fn ap1_atomic_ptr_semantic_slot_survives_exact_value_binding() {
        let a = alloc("rust::main::bb0");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_place_points_to(direct_slot(2), BTreeSet::from([a.clone()]));
        copy_binding(&mut memory, &var(2), var(7));
        assert_eq!(memory.points_to_place(&direct_slot(7)), BTreeSet::from([a]));
    }

    #[test]
    fn ap1_rejects_malformed_or_lookalike_owner_evidence() {
        let a = alloc("rust::main::bb0");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(var(1), BTreeSet::from([a]));
        let mut bad = evidence(RustAtomicPtrCallEvidenceKind::New, "new");
        bad.owner_def_path = "user_crate::AtomicPtr".to_string();
        assert!(!apply_atomic_ptr_identity_summary(
            "main",
            &[arg(1)],
            "Local(_2)",
            &bad,
            &mut memory,
        ));
        assert!(memory.points_to_place(&direct_slot(2)).is_empty());

        bad.owner_def_path = "core::sync::atomic::AtomicPtr".to_string();
        bad.callee_def_path.clear();
        assert!(!apply_atomic_ptr_identity_summary(
            "main",
            &[arg(1)],
            "Local(_2)",
            &bad,
            &mut memory,
        ));
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
            allocation_disposition_evidence,
            atomic_ptr_evidence,
            ..
        }) = &bb.terminator
        {
            let producer_identity_applied = allocation_disposition_evidence
                .as_ref()
                .is_some_and(|evidence| {
                    apply_producer_certified_identity_relation(
                        function,
                        arguments,
                        return_place,
                        evidence,
                        &mut out,
                    )
                });

            let atomic_ptr_identity_applied = !producer_identity_applied
                && atomic_ptr_evidence.as_ref().is_some_and(|evidence| {
                    apply_atomic_ptr_identity_summary(
                        function,
                        arguments,
                        return_place,
                        evidence,
                        &mut out,
                    )
                });

            if !producer_identity_applied && !atomic_ptr_identity_applied {
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
            }
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

fn copy_binding_across_memories(
    source_memory: &AllocationIdentityMemory,
    target_memory: &mut AllocationIdentityMemory,
    source: &ProgramVarId,
    destination: ProgramVarId,
) {
    let allocs = source_memory.points_to(source);
    let refs = source_memory.stack_refs(source);
    let access = source_memory
        .access_bases
        .get(source)
        .cloned()
        .unwrap_or_default();

    target_memory.assign_points_to(destination.clone(), allocs);
    target_memory.assign_stack_refs(destination.clone(), refs);
    target_memory.assign_access_bases(destination.clone(), access);

    let projected_points: Vec<_> = source_memory
        .place_points_to
        .iter()
        .filter(|(place, _)| &place.base == source)
        .map(|(place, allocs)| (place.projection.clone(), allocs.clone()))
        .collect();
    let projected_refs: Vec<_> = source_memory
        .place_stack_refs
        .iter()
        .filter(|(place, _)| &place.base == source)
        .map(|(place, refs)| (place.projection.clone(), refs.clone()))
        .collect();
    let projected_resources: Vec<_> = source_memory
        .place_resources
        .iter()
        .filter(|(place, _)| &place.base == source)
        .map(|(place, resources)| (place.projection.clone(), resources.clone()))
        .collect();

    for (projection, allocs) in projected_points {
        target_memory.assign_place_points_to(
            PlaceId {
                base: destination.clone(),
                projection,
            },
            allocs,
        );
    }
    for (projection, refs) in projected_refs {
        target_memory.assign_place_stack_refs(
            PlaceId {
                base: destination.clone(),
                projection,
            },
            refs,
        );
    }
    for (projection, resources) in projected_resources {
        target_memory.assign_place_resources(
            PlaceId {
                base: destination.clone(),
                projection,
            },
            resources,
        );
    }
}

fn binding_referent_seeds(
    memory: &AllocationIdentityMemory,
    source: &ProgramVarId,
) -> BTreeSet<PlaceId> {
    let mut seeds = memory.stack_refs(source);
    for (place, refs) in &memory.place_stack_refs {
        if &place.base == source {
            seeds.extend(refs.iter().cloned());
        }
    }
    seeds
}

/// Lightweight Rust-call frame transfer used by the standard (non-DEP1)
/// analysis profile.
///
/// Ordinary MIR locals are activation-local.  The legacy implementation cloned
/// the complete caller memory into the callee, which made locals from a prior
/// invocation survive a return/call cycle when the same finite analysis point
/// was revisited.  This transfer keeps the finite k=1 abstraction unchanged
/// while importing only the global heap plus identities reachable from the
/// explicit actual arguments.
fn bind_actuals_to_formals(
    icfg: &GlobalICFGOrdered,
    call: &RustCallMetadata,
    caller_memory: &AllocationIdentityMemory,
) -> AllocationIdentityMemory {
    let Some(function) = icfg.rust_functions.get(&call.callee_function) else {
        // Canonical ICFG validation normally makes this unreachable.  Preserve
        // the previous fail-open fallback rather than losing caller facts.
        return caller_memory.clone();
    };

    let mut callee_memory = AllocationIdentityMemory::default();
    callee_memory.join_heap_from(caller_memory);

    // Heap values can contain stack-place references (for example captured
    // environments).  Preserve only those referents that are reachable from
    // the global heap; do not import the caller's unrelated local environment.
    import_referents(
        caller_memory,
        &mut callee_memory,
        caller_memory.heap_stack_referent_seeds(),
    );

    for formal_index in 0..function.arg_count {
        let Some(argument) = call.arguments.get(formal_index) else {
            break;
        };
        let Some(actual) = first_local_operand(&call.caller_function, &argument.arg) else {
            continue;
        };
        let formal = ProgramVarId::Rust {
            function: call.callee_function.clone(),
            local: (formal_index + 1) as u32,
        };

        copy_binding_across_memories(
            caller_memory,
            &mut callee_memory,
            &actual,
            formal,
        );

        // A reference-valued actual names caller-visible stack places.  Import
        // exactly that reachable referent slice so dereference effects in the
        // callee remain represented without cloning unrelated caller locals.
        import_referents(
            caller_memory,
            &mut callee_memory,
            binding_referent_seeds(caller_memory, &actual),
        );
    }

    callee_memory
}

/// Lightweight Rust-return frame transfer used by the standard (non-DEP1)
/// analysis profile.
///
/// Restore the caller snapshot, retain global heap effects, and re-import only
/// caller-visible stack places reachable from the call's actual arguments.
/// Callee locals survive only when they are explicitly reachable from the
/// returned value.  This prevents one invocation's ordinary locals from
/// becoming input facts of a later invocation while preserving MAY effects
/// observable by the caller.
fn bind_return_to_caller(
    call: &RustCallMetadata,
    callee_memory: &AllocationIdentityMemory,
    caller_snapshot: &AllocationIdentityMemory,
) -> AllocationIdentityMemory {
    let mut out = caller_snapshot.clone();
    out.join_heap_from(callee_memory);

    let mut caller_visible_referents = caller_snapshot.heap_stack_referent_seeds();
    for argument in &call.arguments {
        let Some(actual) = first_local_operand(&call.caller_function, &argument.arg) else {
            continue;
        };
        caller_visible_referents.extend(binding_referent_seeds(caller_snapshot, &actual));
    }
    import_referents(
        callee_memory,
        &mut out,
        caller_visible_referents,
    );

    let callee_return = ProgramVarId::Rust {
        function: call.callee_function.clone(),
        local: 0,
    };

    if let Some(caller_return) = local(&call.caller_function, &call.return_place) {
        let escaped_referents = binding_referent_seeds(callee_memory, &callee_return);
        copy_binding_across_memories(
            callee_memory,
            &mut out,
            &callee_return,
            caller_return,
        );
        // A returned reference/raw aggregate may intentionally keep a callee
        // place reachable.  Preserve that escape rather than dropping it merely
        // because the ordinary callee frame is not imported wholesale.
        import_referents(
            callee_memory,
            &mut out,
            escaped_referents,
        );
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

/// Reconsider already-materialized normal returns when their caller snapshot
/// dependency changes. The k=1 callee input may be unchanged even though a new
/// caller-context continuation has subscribed to that shared callee result.
/// All represented targets of the callsite share the continuation snapshot.
fn notify_materialized_identity_returns(
    icfg: &GlobalICFGOrdered,
    callsite: &str,
    post_states: &BTreeMap<IdentityAnalysisPoint, AllocationIdentityMemory>,
    worklist: &mut BTreeSet<IdentityAnalysisPoint>,
) {
    for call in icfg.rust_calls.iter().filter(|call| call.call_node == callsite) {
        if let Some(function) = icfg.rust_functions.get(&call.callee_function) {
            for node in &function.return_nodes {
                let returned = IdentityAnalysisPoint {
                    node: node.clone(),
                    context: bounded_context(callsite),
                };
                // Do not invent reachability or manufacture a callee summary.
                if post_states.contains_key(&returned) {
                    worklist.insert(returned);
                }
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

/// Opt-in P3-R1P Place/resource-relation gate.  Disabled mode preserves the
/// committed R10 statement-transfer semantics.
pub(crate) fn p3_place_relation_enabled() -> bool {
    std::env::var("CREMA_INTERNAL_DEP1_P3_R1P").as_deref() == Ok("1")
}

/// Return only canonical allocation identities already justified for a value.
/// Stack references are followed because they identify exact stack Places;
/// `access_bases` are intentionally excluded because ERR1 access provenance is
/// not a pointee-identity proof.
fn tracked_resources_of_value(
    memory: &AllocationIdentityMemory,
    source: &ProgramVarId,
) -> BTreeSet<AbstractAllocId> {
    let mut resources = memory.points_to(source);
    for referent in memory.stack_refs(source) {
        resources.extend(canonical_resources_for_place(memory, referent));
    }
    resources
}

/// Cheap structural integrity check for producer evidence.  The semantic type
/// proof is performed in `icfg.rs` while rustc types/DefIds are available; this
/// consumer additionally refuses malformed/mutated evidence instead of
/// silently broadening it.
fn certified_owned_box_source_shape(evidence: &crate::structs::RustPlaceRelationEvidence) -> bool {
    if evidence.source_projection.len() != evidence.transparent_wrapper_depth + 2 {
        return false;
    }
    if !matches!(evidence.source_projection.first(), Some(PlaceProjection::Deref)) {
        return false;
    }
    evidence.source_projection[1..]
        .iter()
        .all(|projection| matches!(projection, PlaceProjection::Field { index: 0 }))
}

/// Apply one producer-certified pointer-carrier assignment.  This transfer is
/// deliberately separate from generic textual `deref_copy` handling: the
/// serialized evidence was established from rustc type/DefId facts at the
/// producer, and consumers never recover it from pretty MIR text.
fn apply_certified_place_relation(
    function: &str,
    stmt: &MirStatement,
    memory: &mut AllocationIdentityMemory,
) -> bool {
    let Some(evidence) = stmt.resource_place_relation_v1.as_ref() else {
        return false;
    };
    let destination = ProgramVarId::Rust {
        function: function.to_string(),
        local: evidence.destination_local,
    };

    match &evidence.kind {
        RustPlaceRelationEvidenceKind::OwnedBoxDataPointerCopyForDeref => {
            if !certified_owned_box_source_shape(evidence) {
                return false;
            }

            // The producer proof says the CopyForDeref result is the raw data
            // pointer carried by the same owned Box value.  Therefore the raw
            // pointer denotes exactly the already-tracked MAY allocation set of
            // the source value/referent.  No new resource ID or stack Place is
            // created.
            let source = ProgramVarId::Rust {
                function: function.to_string(),
                local: evidence.source_local,
            };
            let resources = tracked_resources_of_value(memory, &source);
            clear_destination(memory, &destination);
            memory.assign_points_to(destination.clone(), resources.clone());

            // Materialize the typed pointee relation in the authoritative
            // identity state.  This is intentionally *not* encoded as
            // `place_points_to`: `*destination` denotes the allocation itself;
            // it does not mean the scalar/object stored there is another
            // pointer to that allocation.
            memory.assign_place_resources(
                PlaceId {
                    base: destination,
                    projection: vec![PlaceProjection::Deref],
                },
                resources,
            );
            true
        }
    }
}

/// Opt-in P3-R1D Drop/receiver resource-identity gate.
///
/// This capability is intentionally separate from ordinary R1/R2 and from
/// resource-state effects.  It consumes only P2-certified `mir_drop`
/// activations plus PLACE1's authoritative typed-Place resource relation.
pub(crate) fn p3_drop_receiver_enabled() -> bool {
    std::env::var("CREMA_INTERNAL_DEP1_P3_R1D").as_deref() == Ok("1")
}

fn p2_activation_context_transition_valid(
    activation: &serde_json::Value,
) -> Result<(), String> {
    let key=activation.get("call_key").and_then(serde_json::Value::as_str)
        .ok_or("p2_control_flow_incomplete: call_key")?;
    let parent=activation.get("parent_context").and_then(serde_json::Value::as_array)
        .ok_or("p2_control_flow_incomplete: parent context")?;
    let callee=activation.get("callee_context").and_then(serde_json::Value::as_array)
        .ok_or("p2_control_flow_incomplete: callee context")?;
    let kind=activation.get("context_transition").and_then(serde_json::Value::as_str)
        .unwrap_or("exact_push");
    match kind {
        "exact_push" | "recursive_scc_entry_exact_push" | "dependency_region_entry_exact_push" => {
            let mut expected=parent.clone();
            expected.push(serde_json::Value::String(key.to_owned()));
            if callee != &expected {
                return Err(format!("p2_control_flow_incomplete: invalid {kind} context transition"));
            }
            if kind=="recursive_scc_entry_exact_push" {
                if activation.get("recursive_context_k").and_then(serde_json::Value::as_u64)!=Some(1)
                    || activation.get("recursive_scc_id").and_then(serde_json::Value::as_str).is_none()
                    || activation.get("recursive_entry_context").and_then(serde_json::Value::as_array)!=Some(callee)
                {
                    return Err("p2_control_flow_incomplete: invalid recursive SCC entry certificate".into());
                }
            }
            if kind=="dependency_region_entry_exact_push" {
                if activation.get("dependency_context_k").and_then(serde_json::Value::as_u64)!=Some(1)
                    || activation.get("dependency_entry_context").and_then(serde_json::Value::as_array)!=Some(callee)
                {
                    return Err("p2_control_flow_incomplete: invalid dependency-region entry certificate".into());
                }
            }
        }
        "recursive_scc_k1_replace" => {
            if activation.get("recursive_context_k").and_then(serde_json::Value::as_u64)!=Some(1)
                || activation.get("recursive_scc_id").and_then(serde_json::Value::as_str).is_none()
            {
                return Err("p2_control_flow_incomplete: recursive SCC k=1 certificate missing".into());
            }
            let entry=activation.get("recursive_entry_context").and_then(serde_json::Value::as_array)
                .ok_or("p2_control_flow_incomplete: recursive SCC entry context")?;
            if parent.len()<entry.len() || parent.len()>entry.len()+1 || parent[..entry.len()]!=entry[..] {
                return Err("p2_control_flow_incomplete: recursive SCC parent escaped its exact entry prefix".into());
            }
            let mut expected=entry.clone();
            expected.push(serde_json::Value::String(key.to_owned()));
            if callee != &expected {
                return Err("p2_control_flow_incomplete: invalid SCC-local k=1 replacement".into());
            }
        }
        "dependency_region_k1_replace" => {
            if activation.get("dependency_context_k").and_then(serde_json::Value::as_u64)!=Some(1) {
                return Err("p2_control_flow_incomplete: dependency-region k=1 certificate missing".into());
            }
            let entry=activation.get("dependency_entry_context").and_then(serde_json::Value::as_array)
                .ok_or("p2_control_flow_incomplete: dependency-region entry context")?;
            if parent.len()<entry.len() || parent[..entry.len()]!=entry[..] {
                return Err("p2_control_flow_incomplete: dependency parent escaped its exact entry prefix".into());
            }
            let mut expected=entry.clone();
            expected.push(serde_json::Value::String(key.to_owned()));
            if callee != &expected {
                return Err("p2_control_flow_incomplete: invalid dependency-region k=1 replacement".into());
            }
        }
        other => return Err(format!("p2_control_flow_incomplete: unknown context transition {other}")),
    }

    // Entering a recursive SCC through a dependency-region transition must
    // certify the exact recursive entry as the resulting callee context.  Once
    // inside, recursive_scc_k1_replace validates the bounded recursive suffix
    // separately above.
    if activation.get("recursive_scc_id").and_then(serde_json::Value::as_str).is_some()
        && matches!(kind,"dependency_region_entry_exact_push"|"dependency_region_k1_replace")
    {
        if activation.get("recursive_context_k").and_then(serde_json::Value::as_u64)!=Some(1)
            || activation.get("recursive_entry_context").and_then(serde_json::Value::as_array)!=Some(callee)
        {
            return Err("p2_control_flow_incomplete: dependency transition has invalid recursive SCC entry certificate".into());
        }
    }

    // When a recursive SCC lives inside a dependency region, the recursive
    // quotient may temporarily add its own suffix.  It must nevertheless stay
    // under the producer-certified dependency entry prefix so exiting the SCC
    // can soundly return to the dependency-region k=1 quotient.
    if let Some(dep_k)=activation.get("dependency_context_k").and_then(serde_json::Value::as_u64) {
        if dep_k!=1 { return Err("p2_control_flow_incomplete: unsupported dependency context k".into()); }
        let entry=activation.get("dependency_entry_context").and_then(serde_json::Value::as_array)
            .ok_or("p2_control_flow_incomplete: dependency entry context missing")?;
        if kind=="dependency_region_entry_exact_push" {
            if entry!=callee {
                return Err("p2_control_flow_incomplete: dependency entry is not the exact callee context".into());
            }
        } else if parent.len()<entry.len() || callee.len()<entry.len()
            || parent[..entry.len()]!=entry[..] || callee[..entry.len()]!=entry[..]
        {
            return Err("p2_control_flow_incomplete: bounded dependency context escaped its exact entry prefix".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod r4_context_transition_tests {
    use super::*;

    #[test]
    fn exact_context_transition_remains_exact_push() {
        let a=serde_json::json!({
            "call_key":"K","parent_context":["A"],"callee_context":["A","K"],
            "context_transition":"exact_push"
        });
        assert!(p2_activation_context_transition_valid(&a).is_ok());
    }

    #[test]
    fn recursive_context_transition_accepts_only_entry_prefix_plus_one_key() {
        let a=serde_json::json!({
            "call_key":"K2","parent_context":["OUT","ENTER","K1"],
            "callee_context":["OUT","ENTER","K2"],
            "context_transition":"recursive_scc_k1_replace",
            "recursive_scc_id":"S","recursive_context_k":1,
            "recursive_entry_context":["OUT","ENTER"]
        });
        assert!(p2_activation_context_transition_valid(&a).is_ok());
        let mut bad=a.clone();
        bad["callee_context"]=serde_json::json!(["OUT","ENTER","K1","K2"]);
        assert!(p2_activation_context_transition_valid(&bad).is_err());
    }

    #[test]
    fn dependency_context_transition_preserves_entry_and_replaces_only_internal_suffix() {
        let entry=serde_json::json!({
            "call_key":"ENTER","parent_context":["APP"],"callee_context":["APP","ENTER"],
            "context_transition":"dependency_region_entry_exact_push",
            "dependency_context_k":1,"dependency_entry_context":["APP","ENTER"]
        });
        assert!(p2_activation_context_transition_valid(&entry).is_ok());

        let internal=serde_json::json!({
            "call_key":"K2","parent_context":["APP","ENTER","K1"],
            "callee_context":["APP","ENTER","K2"],
            "context_transition":"dependency_region_k1_replace",
            "dependency_context_k":1,"dependency_entry_context":["APP","ENTER"]
        });
        assert!(p2_activation_context_transition_valid(&internal).is_ok());
        let mut bad=internal.clone();
        bad["callee_context"]=serde_json::json!(["APP","ENTER","K1","K2"]);
        assert!(p2_activation_context_transition_valid(&bad).is_err());

        let recursive_entry=serde_json::json!({
            "call_key":"TO-REC","parent_context":["APP","ENTER","K1"],
            "callee_context":["APP","ENTER","TO-REC"],
            "context_transition":"dependency_region_k1_replace",
            "dependency_context_k":1,"dependency_entry_context":["APP","ENTER"],
            "recursive_scc_id":"S","recursive_context_k":1,
            "recursive_entry_context":["APP","ENTER","TO-REC"]
        });
        assert!(p2_activation_context_transition_valid(&recursive_entry).is_ok());
        let mut bad_recursive_entry=recursive_entry.clone();
        bad_recursive_entry["recursive_entry_context"]=serde_json::json!(["APP","ENTER","WRONG"]);
        assert!(p2_activation_context_transition_valid(&bad_recursive_entry).is_err());
    }

    #[test]
    fn recursive_context_inside_dependency_must_stay_under_dependency_entry_prefix() {
        let a=serde_json::json!({
            "call_key":"REC2","parent_context":["APP","ENTER","TO-REC","REC1"],
            "callee_context":["APP","ENTER","TO-REC","REC2"],
            "context_transition":"recursive_scc_k1_replace",
            "recursive_scc_id":"S","recursive_context_k":1,
            "recursive_entry_context":["APP","ENTER","TO-REC"],
            "dependency_context_k":1,"dependency_entry_context":["APP","ENTER"]
        });
        assert!(p2_activation_context_transition_valid(&a).is_ok());
        let mut escaped=a.clone();
        escaped["callee_context"]=serde_json::json!(["OTHER","REC2"]);
        assert!(p2_activation_context_transition_valid(&escaped).is_err());
    }

}

#[derive(Clone)]
struct DropReceiverPlan {
    activation: serde_json::Value,
    caller_scope: String,
    place: serde_json::Value,
    receiver: ProgramVarId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DropSourceResolution {
    places: BTreeSet<PlaceId>,
    resources: BTreeSet<AbstractAllocId>,
    status: &'static str,
}

fn drop_receiver_plans(
    icfg: &GlobalICFGOrdered,
) -> Result<BTreeMap<String, DropReceiverPlan>, String> {
    let dep = icfg
        .dependency_body_ingestion_v1
        .as_ref()
        .ok_or("p2_control_flow_incomplete: DEP1 absent")?;
    let graph = dep
        .context_execution_graph_v1
        .as_ref()
        .ok_or("p2_control_flow_incomplete: execution graph absent")?;
    let activations = graph
        .get("represented_call_activations")
        .and_then(serde_json::Value::as_array)
        .ok_or("p2_control_flow_incomplete: activation records absent")?;

    let mut plans = BTreeMap::new();
    for activation in activations
        .iter()
        .filter(|activation| activation["activation_kind"] == "mir_drop")
    {
        let text = |key: &str| {
            activation[key]
                .as_str()
                .ok_or_else(|| format!("unresolved_drop_identity: {key}"))
        };
        let call_key = text("call_key")?;
        let caller_instance = text("caller_instance_id")?;
        let callee_instance = text("callee_instance_id")?;

        // P3-R1D consumes the exact P2 binding; it never resolves a destructor
        // again by type/name at identity-analysis time.
        let bindings: Vec<_> = dep
            .exact_call_bindings
            .iter()
            .filter(|binding| {
                binding["call_key"] == activation["call_key"]
                    && binding["activation_kind"] == "mir_drop"
                    && binding["caller_instance_id"] == activation["caller_instance_id"]
                    && binding["resolved_instance_id"] == activation["callee_instance_id"]
                    && binding["body_status"] == "represented_body"
            })
            .collect();
        let [binding] = bindings.as_slice() else {
            return Err(format!(
                "unresolved_drop_identity: nonunique exact binding for {call_key}"
            ));
        };

        let bodies: Vec<_> = dep
            .body_statuses
            .iter()
            .filter(|body| {
                body["instance_id"] == activation["callee_instance_id"]
                    && body["body_status"] == "represented_body"
                    && body["body_provider_kind"] == "compiler_generated_instance_mir"
            })
            .collect();
        let [body] = bodies.as_slice() else {
            return Err(format!(
                "unresolved_receiver: nonunique compiler-generated body for {callee_instance}"
            ));
        };

        let receiver_proof = &body["drop_receiver_v1"];
        if receiver_proof["source"] != "rustc_instance_mir_args_iter_local_decls" {
            return Err("unresolved_receiver: compiler receiver proof missing".into());
        }
        let arguments = receiver_proof["arguments"]
            .as_array()
            .ok_or("unresolved_receiver: compiler argument proof missing")?;
        let [argument] = arguments.as_slice() else {
            return Err("unsupported_receiver: DropGlue must have exactly one MIR argument".into());
        };
        if receiver_proof["arg_count"].as_u64() != Some(1)
            || argument["raw_mutable_pointer"] != true
        {
            return Err("unsupported_receiver: compiler signature is not one mutable raw pointer".into());
        }
        let receiver_local = argument["local"]
            .as_u64()
            .ok_or("unresolved_receiver: compiler local missing")?;

        let entry = text("callee_entry_state")?;
        let scopes: Vec<_> = icfg
            .rust_functions
            .iter()
            .filter(|(_, function)| function.entry_node == entry)
            .collect();
        let [(callee_scope, _)] = scopes.as_slice() else {
            return Err("unresolved_receiver: callee entry scope is not unique".into());
        };
        let caller_scope = rust_function_from_node(text("caller_call_state")?)
            .ok_or("unresolved_drop_identity: caller scope")?
            .to_string();

        p2_activation_context_transition_valid(activation)?;
        let proxy = text("dummy_call_state")?;
        if !icfg
            .icfg_edges
            .iter()
            .any(|edge| edge.source == proxy && edge.destination == entry)
        {
            return Err("p2_control_flow_incomplete: certified Drop entry edge absent".into());
        }

        let place = binding["drop_place_v1"]["typed_place_v1"].clone();
        if place["source"] != "rustc_mir_Drop_Place"
            || place["local"].as_u64().is_none()
            || !place["projection"].is_array()
        {
            return Err("unresolved_drop_identity: typed Drop Place proof absent".into());
        }
        let receiver = ProgramVarId::Rust {
            function: (*callee_scope).clone(),
            local: receiver_local as u32,
        };
        if plans
            .insert(
                proxy.to_string(),
                DropReceiverPlan {
                    activation: activation.clone(),
                    caller_scope,
                    place,
                    receiver,
                },
            )
            .is_some()
        {
            return Err(format!(
                "p2_control_flow_incomplete: duplicate Drop activation proxy for {caller_instance}"
            ));
        }
    }
    Ok(plans)
}

fn typed_drop_place(plan: &DropReceiverPlan) -> Result<PlaceId, &'static str> {
    let local = plan.place["local"].as_u64().ok_or("unresolved_identity")? as u32;
    let mut projection = Vec::new();
    for component in plan.place["projection"]
        .as_array()
        .ok_or("unresolved_identity")?
    {
        match component["kind"].as_str() {
            Some("field") => projection.push(PlaceProjection::Field {
                index: component["index"].as_u64().ok_or("unsupported_place")? as u32,
            }),
            Some("deref") => projection.push(PlaceProjection::Deref),
            _ => return Err("unsupported_place"),
        }
    }
    Ok(PlaceId {
        base: ProgramVarId::Rust {
            function: plan.caller_scope.clone(),
            local,
        },
        projection,
    })
}

/// Resolve the exact storage Place(s) named by a compiler-certified MIR Drop.
///
/// A terminal dereference may be justified directly by PLACE1's authoritative
/// `Place -> MAY {resource}` relation even when no stack Place exists for the
/// heap object.  Deeper projections still require exact stack-reference edges;
/// this preserves PLACE1's fail-closed heap-shape boundary.
fn canonical_resources_in_exact_place_subtree(
    memory: &AllocationIdentityMemory,
    root: &PlaceId,
) -> BTreeSet<AbstractAllocId> {
    let mut resources = canonical_resources_for_place(memory, root.clone());
    let mut descendants = BTreeSet::new();
    for place in memory
        .place_points_to
        .keys()
        .chain(memory.place_stack_refs.keys())
        .chain(memory.place_resources.keys())
    {
        if place.base == root.base
            && place.projection.len() >= root.projection.len()
            && place.projection[..root.projection.len()] == root.projection[..]
        {
            descendants.insert(place.clone());
        }
    }
    for place in descendants {
        resources.extend(canonical_resources_for_place(memory, place));
    }
    resources
}

fn resolve_drop_source(
    plan: &DropReceiverPlan,
    memory: &AllocationIdentityMemory,
) -> DropSourceResolution {
    let query = match typed_drop_place(plan) {
        Ok(query) => query,
        Err(status) => {
            return DropSourceResolution {
                places: BTreeSet::new(),
                resources: BTreeSet::new(),
                status,
            }
        }
    };

    let place_resource = resolve_resource_place(memory, &query);
    let mut places = BTreeSet::from([PlaceId {
        base: query.base.clone(),
        projection: Vec::new(),
    }]);

    for (position, component) in query.projection.iter().enumerate() {
        let terminal = position + 1 == query.projection.len();
        match component {
            PlaceProjection::Field { index } => {
                places = places
                    .into_iter()
                    .map(|mut place| {
                        place.projection.push(PlaceProjection::Field { index: *index });
                        place
                    })
                    .collect();
            }
            PlaceProjection::Deref => {
                let mut referents = BTreeSet::new();
                for place in &places {
                    referents.extend(memory.stack_refs_place(place));
                }
                if !referents.is_empty() {
                    places = referents;
                    continue;
                }

                // PLACE1 may authoritatively identify a terminal heap Place
                // (the saved Box `_6 -> *_6` blocker) without fabricating a
                // stack referent.  The exact MIR Drop Place itself is the
                // storage identity used for the DropGlue receiver relation.
                if terminal
                    && matches!(&place_resource, ResourcePlaceResolution::Resolved(_))
                {
                    places = BTreeSet::from([query.clone()]);
                    continue;
                }
                return DropSourceResolution {
                    places: BTreeSet::new(),
                    resources: BTreeSet::new(),
                    status: "unresolved_identity",
                };
            }
            PlaceProjection::Index { .. } | PlaceProjection::Opaque { .. } => {
                return DropSourceResolution {
                    places: BTreeSet::new(),
                    resources: BTreeSet::new(),
                    status: "unsupported_place",
                }
            }
        }
    }

    let mut resources = match place_resource {
        ResourcePlaceResolution::Resolved(resources) => resources,
        ResourcePlaceResolution::UnresolvedIdentity => BTreeSet::new(),
        ResourcePlaceResolution::UnsupportedPlace => {
            return DropSourceResolution {
                places: BTreeSet::new(),
                resources: BTreeSet::new(),
                status: "unsupported_place",
            }
        }
    };
    for place in &places {
        resources.extend(canonical_resources_in_exact_place_subtree(memory, place));
    }

    // An exact direct/field Place with no tracked relation is a proved
    // non-resource in this identity abstraction.  A dereference without a
    // proof never reaches this point; it is unresolved above.
    let status = if resources.is_empty() {
        "no_tracked_resource"
    } else {
        "resource_binding_available"
    };
    DropSourceResolution {
        places,
        resources,
        status,
    }
}

/// Copy only represented descendants of the exact dropped storage into the
/// receiver-local namespace.  This is a structural rebase, not heap-shape
/// inference: only relations already present below an exact source prefix are
/// copied, and canonical allocation IDs are never changed or created.
fn import_drop_subtree(
    caller: &AllocationIdentityMemory,
    callee: &mut AllocationIdentityMemory,
    source: &PlaceId,
    receiver: &ProgramVarId,
) {
    let prefix_len = source.projection.len();
    for (place, allocs) in &caller.place_points_to {
        if place.base == source.base
            && place.projection.len() >= prefix_len
            && place.projection[..prefix_len] == source.projection[..]
        {
            let suffix = &place.projection[prefix_len..];
            if suffix.is_empty() {
                continue;
            }
            callee.assign_place_points_to(
                PlaceId {
                    base: receiver.clone(),
                    projection: suffix.to_vec(),
                },
                allocs.clone(),
            );
        }
    }
    for (place, refs) in &caller.place_stack_refs {
        if place.base == source.base
            && place.projection.len() >= prefix_len
            && place.projection[..prefix_len] == source.projection[..]
        {
            let suffix = &place.projection[prefix_len..];
            if suffix.is_empty() {
                continue;
            }
            callee.assign_place_stack_refs(
                PlaceId {
                    base: receiver.clone(),
                    projection: suffix.to_vec(),
                },
                refs.clone(),
            );
        }
    }
    for (place, resources) in &caller.place_resources {
        if place.base == source.base
            && place.projection.len() >= prefix_len
            && place.projection[..prefix_len] == source.projection[..]
        {
            let mut projection = vec![PlaceProjection::Deref];
            projection.extend(place.projection[prefix_len..].iter().cloned());
            callee.assign_place_resources(
                PlaceId {
                    base: receiver.clone(),
                    projection,
                },
                resources.clone(),
            );
        }
    }
}

fn drop_callee_context(plan: &DropReceiverPlan) -> Option<Vec<String>> {
    plan.activation["caller_call_state"]
        .as_str()
        .map(bounded_context)
}

fn drop_same_context_successors(
    successors: &BTreeSet<String>,
    matched_cross_body_unwind_successors: &BTreeSet<String>,
) -> BTreeSet<String> {
    successors
        .difference(matched_cross_body_unwind_successors)
        .cloned()
        .collect()
}

fn transfer_drop_receiver(
    plan: &DropReceiverPlan,
    caller: &AllocationIdentityMemory,
) -> (AllocationIdentityMemory, serde_json::Value) {
    let resolution = resolve_drop_source(plan, caller);
    let mut callee = AllocationIdentityMemory::default();
    callee.join_heap_from(caller);
    import_referents(caller, &mut callee, caller.heap_stack_referent_seeds());

    if matches!(resolution.status, "resource_binding_available" | "no_tracked_resource") {
        for place in &resolution.places {
            import_referents(caller, &mut callee, BTreeSet::from([place.clone()]));
            import_drop_subtree(caller, &mut callee, place, &plan.receiver);
        }
        callee.assign_points_to(plan.receiver.clone(), resolution.resources.clone());
        callee.assign_stack_refs(plan.receiver.clone(), resolution.places.clone());
        if !resolution.resources.is_empty() {
            callee.assign_place_resources(
                PlaceId {
                    base: plan.receiver.clone(),
                    projection: vec![PlaceProjection::Deref],
                },
                resolution.resources.clone(),
            );
        }
    }

    let activation = &plan.activation;
    let record = serde_json::json!({
        "call_key": activation["call_key"],
        "activation_kind": "mir_drop",
        "caller_instance_id": activation["caller_instance_id"],
        "concrete_callee_instance": activation["callee_instance_id"],
        "caller_execution_context": activation["parent_context"],
        "callee_execution_context": activation["callee_context"],
        "caller_call_state": activation["caller_call_state"],
        "dummy_call_state": activation["dummy_call_state"],
        "dummy_ret_state": activation["dummy_ret_state"],
        "callee_entry_state": activation["callee_entry_state"],
        "dropped_place": plan.place,
        "resolved_places": resolution.places,
        "receiver": plan.receiver,
        "resources": resolution.resources,
        "status": resolution.status,
        "certainty": "may",
        "relation_kind": "drop_place_to_glue_receiver",
        "provenance": "rustc_typed_Drop_Place_plus_instance_mir_receiver_plus_PLACE1_v1",
        "resource_effects": "not_evaluated_R1D",
        "ownership_liveness": "not_inferred",
    });
    (callee, record)
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
        // P2 Drop is control-only; receiver continuity belongs to P3-R1D.
        if a.get("activation_kind").and_then(serde_json::Value::as_str) == Some("mir_drop") { continue; }
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
        p2_activation_context_transition_valid(a)?;
        let entry = text("callee_entry_state")?;
        let scopes: Vec<_> = icfg.rust_functions.iter().filter(|(_,f)| f.entry_node == entry).collect();
        let [(callee_scope, metadata)] = scopes.as_slice() else { return Err("unresolved_formal: entry scope".into()); };
        let caller_scope = rust_function_from_node(text("caller_call_state")?).ok_or("unresolved_actual: caller scope")?.to_string();
        let actuals = binding.get("actual_operands_v1").and_then(serde_json::Value::as_array).ok_or("unresolved_actual: compiler operand proof absent")?.clone();
        let formal_proof = &body["formal_arguments_v1"];
        let formals = formal_proof["arguments"].as_array().ok_or("unresolved_formal: compiler locals proof absent")?.clone();
        let is_spawn_callback = a.get("activation_kind").and_then(serde_json::Value::as_str)
            == Some("higher_order_callback");
        if is_spawn_callback {
            if a.get("callback_semantics").and_then(serde_json::Value::as_str) != Some("spawn_may_once")
                || binding.get("callback_semantics").and_then(serde_json::Value::as_str) != Some("spawn_may_once")
                || formal_proof["abi"] != "rustc_closure_body_v1"
                || actuals.len() != 1
                || actuals.first().and_then(|v|v.get("value_shape")).and_then(serde_json::Value::as_str) != Some("closure_environment")
            {
                return Err("unresolved_formal: uncertified higher-order closure environment boundary".into());
            }
        }
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
    // Heap contents are global abstract store, not caller-local environment.
    // Preserve them across a represented Rust call while keeping ordinary
    // local identity import restricted to the certified actual/formal slice.
    callee.join_heap_from(caller);
    import_referents(caller, &mut callee, caller.heap_stack_referent_seeds());
    let mut records = Vec::new();
    for (i,(actual,formal)) in plan.actuals.iter().zip(&plan.formals).enumerate() {
        let destination = ProgramVarId::Rust { function:plan.callee_scope.clone(),local:(i+1) as u32 };
        let source = actual["actual_local"].as_u64().map(|local| ProgramVarId::Rust {function:plan.caller_scope.clone(),local:local as u32});
        let shape = actual["value_shape"].as_str().unwrap_or("unsupported_value_shape");
        let supported = matches!(shape,"raw_pointer"|"shared_reference"|"mutable_reference"|"owned_box"|"scalar"|"aggregate"|"closure_environment");
        let is_spawn_callback = plan.activation.get("activation_kind").and_then(serde_json::Value::as_str)
            == Some("higher_order_callback")
            && plan.activation.get("callback_semantics").and_then(serde_json::Value::as_str) == Some("spawn_may_once");
        let boundary_supported = plan.abi == "Rust"
            || (is_spawn_callback && plan.abi == "rustc_closure_body_v1" && shape == "closure_environment");
        let status = if !boundary_supported || !supported || shape != formal["value_shape"].as_str().unwrap_or("")
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
            for (p,resources) in &caller.place_resources { if &p.base == source {
                callee.assign_place_resources(PlaceId{base:destination.clone(),projection:p.projection.clone()},resources.clone());
            }}
            let projected_identity = caller.place_points_to.iter().any(|(p,a)| &p.base == source && !a.is_empty())
                || caller.place_stack_refs.iter().any(|(p,r)| &p.base == source && !r.is_empty())
                || caller.place_resources.iter().any(|(p,r)| &p.base == source && !r.is_empty());
            if caller.event_allocations(source).is_empty()
                && caller.stack_refs(source).is_empty()
                && !projected_identity
            {
                "unresolved_actual"
            } else { "resource_binding_available" }
        } else { "unresolved_actual" };
        let resources = source.as_ref().filter(|_| status=="resource_binding_available")
            .map(|s| {
                let mut resources = caller.event_allocations(s);
                for (p, allocs) in &caller.place_points_to {
                    if &p.base == s { resources.extend(allocs.iter().cloned()); }
                }
                for (p, allocs) in &caller.place_resources {
                    if &p.base == s { resources.extend(allocs.iter().cloned()); }
                }
                resources
            }).unwrap_or_default();
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

/// R2 is separately opt-in so the accepted R1 transfer remains reproducible.
pub(crate) fn p3_return_continuity_enabled() -> bool {
    std::env::var("CREMA_INTERNAL_DEP1_P3_R2").as_deref() == Ok("1")
}

#[derive(Clone)]
struct ReturnResourcePlan {
    entry: ActualFormalPlan,
    relation: serde_json::Value,
    destination: Option<PlaceId>,
    return_shape: String,
}

type ReturnResourcePlans = BTreeMap<String, Vec<ReturnResourcePlan>>;

fn insert_return_resource_plan(
    plans: &mut ReturnResourcePlans,
    source: &str,
    plan: ReturnResourcePlan,
) -> Result<(), String> {
    let caller_call_state = plan.entry.activation["caller_call_state"]
        .as_str()
        .ok_or("p2_control_flow_incomplete: return plan caller state absent")?;
    let dummy_ret_state = plan.relation["dummy_ret_state"]
        .as_str()
        .ok_or("p2_control_flow_incomplete: return plan proxy absent")?;
    let bucket = plans.entry(source.to_string()).or_default();
    if bucket.iter().any(|existing| {
        existing.entry.activation["caller_call_state"].as_str() == Some(caller_call_state)
            && existing.relation["dummy_ret_state"].as_str() == Some(dummy_ret_state)
    }) {
        return Err("p2_control_flow_incomplete: duplicate normal-return activation".into());
    }
    bucket.push(plan);
    bucket.sort_by(|left, right| {
        (
            left.entry.activation["caller_call_state"].as_str().unwrap_or_default(),
            left.relation["dummy_ret_state"].as_str().unwrap_or_default(),
        )
            .cmp(&(
                right.entry.activation["caller_call_state"].as_str().unwrap_or_default(),
                right.relation["dummy_ret_state"].as_str().unwrap_or_default(),
            ))
    });
    Ok(())
}

fn matching_return_resource_plans<'a>(
    plans: &'a ReturnResourcePlans,
    source: &str,
    context: &[String],
) -> Vec<&'a ReturnResourcePlan> {
    let Some(call_state) = context.last().map(String::as_str) else {
        return Vec::new();
    };
    plans
        .get(source)
        .into_iter()
        .flatten()
        .filter(|plan| {
            plan.entry.activation["caller_call_state"].as_str() == Some(call_state)
        })
        .collect()
}

fn return_resource_plans(icfg: &GlobalICFGOrdered, entries: &BTreeMap<String, ActualFormalPlan>)
    -> Result<ReturnResourcePlans,String> {
    let dep = icfg.dependency_body_ingestion_v1.as_ref().ok_or("p2_control_flow_incomplete")?;
    let mut plans = BTreeMap::new();
    for entry in entries.values() {
        let a = &entry.activation;
        let call = dep.exact_call_bindings.iter().find(|b|b["call_key"]==a["call_key"])
            .ok_or("unresolved_destination: compiler call absent")?;
        let body = dep.body_statuses.iter().find(|b|b["instance_id"]==a["callee_instance_id"])
            .ok_or("unresolved_return: compiler body absent")?;
        if a.get("activation_kind").and_then(serde_json::Value::as_str) == Some("higher_order_callback") {
            if a.get("callback_semantics").and_then(serde_json::Value::as_str) != Some("spawn_may_once")
                || call.get("callback_semantics").and_then(serde_json::Value::as_str) != Some("spawn_may_once")
            {
                return Err("p2_control_flow_incomplete: unsupported higher-order callback return semantics".into());
            }
            let relations = a["normal_return_relations"].as_array()
                .ok_or("p2_control_flow_incomplete: callback normal relations absent")?;
            if relations.is_empty() {
                return Err("p2_control_flow_incomplete: represented callback has no normal-return relation".into());
            }
            for relation in relations {
                let source = relation["callee_return_state"].as_str().ok_or("unresolved_return: callback exit absent")?;
                let proxy = relation["dummy_ret_state"].as_str().ok_or("unresolved_destination: callback return proxy absent")?;
                let next = relation["continuation_state"].as_str().ok_or("unresolved_destination: callback continuation absent")?;
                if relation["dummy_ret_state"] != a["dummy_ret_state"]
                    || relation["continuation_state"] != a["continuation_state"]
                    || !icfg.icfg_edges.iter().any(|e|e.source==source && e.destination==proxy && e.label.as_deref()==Some("Rust Return -> dummyRet"))
                    || !icfg.icfg_edges.iter().any(|e|e.source==proxy && e.destination==next && e.label.as_deref()==Some("dummyRet -> Rust Continuation"))
                    || !icfg.ordered_nodes.iter().any(|(id,node)|id==source && matches!(node,GlobalICFGNode::Mir(MirBasicBlock{terminator:Some(MirTerminator::Return{..}),..})))
                {
                    return Err("p2_control_flow_incomplete: exact callback normal-return relation absent".into());
                }
                let plan = ReturnResourcePlan {
                    entry:entry.clone(),
                    relation:relation.clone(),
                    destination:None,
                    return_shape:"detached_callback_no_value_to_provider".into(),
                };
                insert_return_resource_plan(&mut plans, source, plan)?;
            }
            continue;
        }
        let proof = &call["return_destination_v1"];
        if proof["has_normal_return"] != !a["return_target"].is_null()
            || proof["normal_target"].as_u64().map(|bb|format!("bb{bb}")) != a["return_target"].as_str().map(str::to_string) {
            return Err("p2_control_flow_incomplete: compiler normal target mismatch".into());
        }
        if body["return_place_v1"]["local"].as_u64()!=Some(0) {
            return Err("unresolved_return: MIR RETURN_PLACE proof absent".into());
        }
        let destination = if proof["supported_projection"]==true {
            let local = proof["local"].as_u64().ok_or("unresolved_destination: local absent")?;
            let projection: Vec<PlaceProjection> = serde_json::from_value(proof["projection"].clone())
                .map_err(|_|"unresolved_destination: projection absent")?;
            if projection.iter().any(|p| !matches!(p,PlaceProjection::Field{..})) {
                return Err("unsupported_value_shape: uncertified destination projection".into());
            }
            Some(PlaceId {base:ProgramVarId::Rust{function:entry.caller_scope.clone(),local:local as u32},projection})
        } else { None };
        let relations = a["normal_return_relations"].as_array().ok_or("p2_control_flow_incomplete: normal relations absent")?;
        for relation in relations {
            let source = relation["callee_return_state"].as_str().ok_or("unresolved_return: exit absent")?;
            let proxy = relation["dummy_ret_state"].as_str().ok_or("unresolved_destination: return proxy absent")?;
            let next = relation["continuation_state"].as_str().ok_or("unresolved_destination: continuation absent")?;
            if a["return_target"].is_null() || relation["dummy_ret_state"]!=a["dummy_ret_state"]
                || relation["continuation_state"]!=a["continuation_state"]
                || !icfg.icfg_edges.iter().any(|e|e.source==source && e.destination==proxy && e.label.as_deref()==Some("Rust Return -> dummyRet"))
                || !icfg.icfg_edges.iter().any(|e|e.source==proxy && e.destination==next && e.label.as_deref()==Some("dummyRet -> Rust Continuation"))
                || !icfg.ordered_nodes.iter().any(|(id,node)|id==source && matches!(node,GlobalICFGNode::Mir(MirBasicBlock{terminator:Some(MirTerminator::Return{..}),..}))) {
                return Err("p2_control_flow_incomplete: exact normal-return relation absent".into());
            }
            let plan = ReturnResourcePlan {entry:entry.clone(),relation:relation.clone(),destination:destination.clone(),
                return_shape:body["return_place_v1"]["value_shape"].as_str().unwrap_or("unsupported_value_shape").into()};
            insert_return_resource_plan(&mut plans, source, plan)?;
        }
    }
    Ok(plans)
}

/// Value return only: restore the caller snapshot, then overwrite the exact
/// destination from _0. Do not join the complete callee environment or effects.
fn transfer_return_resource(plan: &ReturnResourcePlan, callee: &AllocationIdentityMemory,
    caller: &AllocationIdentityMemory) -> (AllocationIdentityMemory,serde_json::Value) {
    let source = ProgramVarId::Rust {function:plan.entry.callee_scope.clone(),local:0};
    let resources = callee.event_allocations(&source);
    let refs = callee.stack_refs(&source);
    let access = callee.access_bases.get(&source).cloned().unwrap_or_default();
    let mut out = caller.clone();
    // Return-value rebasing remains narrow, but heap effects are global store
    // effects and must survive the call boundary.
    out.join_heap_from(callee);
    if plan.entry.activation.get("activation_kind").and_then(serde_json::Value::as_str)
        == Some("higher_order_callback")
    {
        // A spawned callback does not return a value into the provider Call's
        // MIR destination.  Restore the spawning caller's local environment
        // and retain only global heap effects produced along this MAY callback
        // branch.  This is the counterpart of the direct-continuation branch
        // retained by P2 and prevents closure locals from leaking into caller
        // scope while preserving callback frees/writes.
        return (out, serde_json::json!({
            "call_key":plan.entry.activation["call_key"],
            "caller_instance_id":plan.entry.activation["caller_instance_id"],
            "concrete_callee_instance":plan.entry.activation["callee_instance_id"],
            "caller_call_state":plan.entry.activation["caller_call_state"],
            "callee_return_state":plan.relation["callee_return_state"],
            "dummy_ret_state":plan.relation["dummy_ret_state"],
            "continuation_state":plan.relation["continuation_state"],
            "relation_kind":"detached_callback_heap_effect_return",
            "callback_semantics":"spawn_may_once",
            "return_value_to_provider":false,
            "status":"heap_effects_joined_into_caller_snapshot",
            "certainty":"may",
            "provenance":"P2_exact_callback_return_plus_global_heap_store_v1"
        }));
    }
    let projected_identity = callee.place_points_to.iter().any(|(place, allocs)| {
        place.base == source && !allocs.is_empty()
    }) || callee.place_stack_refs.iter().any(|(place, targets)| {
        place.base == source && !targets.is_empty()
    });
    let mut status = if plan.destination.is_none() {"unsupported_destination_shape"}
        else if !matches!(plan.return_shape.as_str(),"raw_pointer"|"shared_reference"|"mutable_reference"|"owned_box"|"scalar"|"unit"|"aggregate") || plan.entry.abi!="Rust" {"unsupported_value_shape"}
        else if resources.is_empty() && refs.is_empty() {
            if matches!(plan.return_shape.as_str(),"scalar"|"unit") {"no_tracked_resource"}
            else if plan.return_shape=="aggregate" && projected_identity {"resource_binding_available"}
            else {"unresolved_return"}
        } else {"resource_binding_available"};
    // Returning a reference is not permission to import changes to preexisting
    // caller places. Such changes require the later side-effect gate.
    let mut pending: Vec<_> = refs.iter().cloned().collect();
    for (place,targets) in &callee.place_stack_refs { if place.base==source {pending.extend(targets.iter().cloned());} }
    let mut seen = BTreeSet::new();
    while let Some(place)=pending.pop() {
        if !seen.insert(place.clone()) {continue;}
        let callee_owned = matches!(&place.base,ProgramVarId::Rust{function,..} if function==&plan.entry.callee_scope);
        if !callee_owned && (callee.points_to_place(&place)!=caller.points_to_place(&place)
            || callee.stack_refs_place(&place)!=caller.stack_refs_place(&place)
            || (place.projection.is_empty() && callee.access_bases.get(&place.base)!=caller.access_bases.get(&place.base))) {
            status="unresolved_return";
        }
        pending.extend(callee.stack_refs_place(&place));
    }
    if let Some(destination)=&plan.destination {
        if !destination.projection.is_empty() && !access.is_empty() {status="unsupported_value_shape";}
        // Every supported assignment overwrites its destination, including a
        // positive nonresource result or an incomplete/unknown result.
        if destination.projection.is_empty() {out.forget_var(&destination.base);}
        else {out.assign_place_points_to(destination.clone(),BTreeSet::new());out.assign_place_stack_refs(destination.clone(),BTreeSet::new());}
        if status=="resource_binding_available" {
            let mut referents = AllocationIdentityMemory::default();
            let seeds = seen.clone();
            import_referents(callee,&mut referents,seeds);
            let owned = |v:&ProgramVarId| matches!(v,ProgramVarId::Rust{function,..} if function==&plan.entry.callee_scope);
            referents.points_to.retain(|v,_|owned(v));
            referents.stack_refs.retain(|v,_|owned(v));
            referents.access_bases.retain(|v,_|owned(v));
            referents.place_points_to.retain(|p,_|owned(&p.base));
            referents.place_stack_refs.retain(|p,_|owned(&p.base));
            // The equality guard above permits caller-place identity copies,
            // not changes to caller state. Only return-reachable places enter.
            out=out.join(&referents);
            if destination.projection.is_empty() {
                out.assign_points_to(destination.base.clone(),callee.points_to(&source));
                out.assign_access_bases(destination.base.clone(),access);
                out.assign_stack_refs(destination.base.clone(),refs);
            } else {
                out.assign_place_points_to(destination.clone(),callee.points_to(&source));
                out.assign_place_stack_refs(destination.clone(),refs);
            }
            for (place,allocs) in &callee.place_points_to {if place.base==source {
                let mut projection=destination.projection.clone();projection.extend(place.projection.clone());
                out.assign_place_points_to(PlaceId{base:destination.base.clone(),projection},allocs.clone());
            }}
            for (place,targets) in &callee.place_stack_refs {if place.base==source {
                let mut projection=destination.projection.clone();projection.extend(place.projection.clone());
                out.assign_place_stack_refs(PlaceId{base:destination.base.clone(),projection},targets.clone());
            }}
        }
    }
    let a=&plan.entry.activation;
    let record=serde_json::json!({"call_key":a["call_key"],"caller_instance_id":a["caller_instance_id"],
        "concrete_callee_instance":a["callee_instance_id"],"caller_execution_context":a["parent_context"],
        "callee_execution_context":a["callee_context"],"caller_call_state":a["caller_call_state"],
        "callee_return_variable":source,"caller_destination":plan.destination,"return_shape":plan.return_shape,
        "normal_return_relation":plan.relation,"resources":resources,"status":status,"certainty":"may",
        "relation_kind":"callee_return_to_caller_destination","provenance":"rustc_RETURN_PLACE_and_Call_destination_on_P2_matched_normal_return_v1"});
    (out,record)
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

    let r1d_enabled = p3_drop_receiver_enabled();
    let r1d_plans = if r1d_enabled {
        drop_receiver_plans(icfg)
    } else {
        Ok(BTreeMap::new())
    };
    let mut r1d_records = BTreeMap::<String, serde_json::Value>::new();
    let mut r1d_entry_points = BTreeSet::new();
    let mut r1d_snapshots =
        BTreeMap::<String, (Vec<String>, AllocationIdentityMemory)>::new();

    let r1_enabled = p3_actual_formal_enabled();
    let r1_plans = if r1_enabled { actual_formal_plans(icfg) } else { Ok(BTreeMap::new()) };
    let r2_enabled = p3_return_continuity_enabled();
    let r2_plans = if r2_enabled {r1_plans.as_ref().map_err(Clone::clone).and_then(|p|return_resource_plans(icfg,p))} else {Ok(BTreeMap::new())};
    let mut graph_reachable=BTreeSet::new();
    let mut pending=vec![entry.to_string()];
    while let Some(node)=pending.pop() {if graph_reachable.insert(node.clone()) {pending.extend(succs.get(&node).into_iter().flatten().cloned());}}
    let mut r2_records = BTreeMap::<String,serde_json::Value>::new();
    let mut r2_pre_states = BTreeMap::<String,IdentityMemoryDump>::new();
    let mut r2_post_points = BTreeSet::new();
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

        if let Some(plan) = r1d_plans
            .as_ref()
            .ok()
            .and_then(|plans| plans.get(&point.node))
        {
            r1d_snapshots
                .entry(point.node.clone())
                .and_modify(|(_, memory)| *memory = memory.join(&post))
                .or_insert((point.context.clone(), post.clone()));
            let (callee_input, record) = transfer_drop_receiver(plan, &post);
            r1d_records.insert(point.node.clone(), record);
            let callee_point = IdentityAnalysisPoint {
                node: plan.activation["callee_entry_state"]
                    .as_str()
                    .expect("validated Drop entry")
                    .to_string(),
                context: bounded_context(
                    plan.activation["caller_call_state"]
                        .as_str()
                        .expect("validated Drop caller state"),
                ),
            };
            r1d_entry_points.insert(callee_point.clone());
            if join_input(&mut in_states, callee_point.clone(), callee_input) {
                worklist.insert(callee_point);
            }
            continue;
        }

        if let Some(plan) = r1_plans.as_ref().ok().and_then(|plans|plans.get(&point.node)) {
            if let Some(call) = rust_call_for_dummy_call(icfg, &point.node) {
                let continuation_changed = continuations
                    .entry((call.call_node.clone(), point.context.clone()))
                    .or_default()
                    .join_snapshot(&point.context, &post);
                if continuation_changed {
                    notify_materialized_identity_returns(
                        icfg, &call.call_node, &post_states, &mut worklist,
                    );
                }
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
            let continuation_changed = continuations
                .entry(key)
                .or_default()
                .join_snapshot(&point.context, &post);
            if continuation_changed {
                notify_materialized_identity_returns(
                    icfg, &call.call_node, &post_states, &mut worklist,
                );
            }

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

        // R1D return routing is edge-sensitive.  A P2 unwind relation can
        // originate at a callee block that also has a normal intra-callee
        // successor (for example Box DropGlue calling `<Box as Drop>::drop`).
        // In that case the unwind branch must pop to the caller context without
        // consuming the normal callee path.  Only a matched normal Drop return
        // is terminal for the current callee activation.
        let mut r1d_matched_normal_return = false;
        let mut r1d_cross_body_unwind_successors = BTreeSet::new();
        if r1d_enabled {
            if let Ok(plans) = &r1d_plans {
                for (proxy, plan) in plans {
                    let Some((caller_context, snapshot)) = r1d_snapshots.get(proxy) else {
                        continue;
                    };
                    // A compiler-generated DropGlue body can be reused by many
                    // concrete Drop activations.  Node identity alone therefore
                    // cannot select a return continuation: require the exact
                    // k=1 callee context created for this P2 activation before
                    // restoring its caller snapshot.
                    if drop_callee_context(plan).as_ref() != Some(&point.context) {
                        continue;
                    }
                    for relation in plan.activation["normal_return_relations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if relation["callee_return_state"] == point.node {
                            let next = IdentityAnalysisPoint {
                                node: relation["dummy_ret_state"]
                                    .as_str()
                                    .expect("P2 Drop normal return proxy")
                                    .to_string(),
                                context: caller_context.clone(),
                            };
                            let mut returned = snapshot.clone();
                            returned.join_heap_from(&post);
                            if join_input(&mut in_states, next.clone(), returned) {
                                worklist.insert(next);
                            }
                            r1d_matched_normal_return = true;
                        }
                    }
                    for relation in plan.activation["unwind_return_relations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if relation["callee_unwind_state"] == point.node {
                            let next = IdentityAnalysisPoint {
                                node: relation["caller_unwind_state"]
                                    .as_str()
                                    .expect("P2 Drop unwind continuation")
                                    .to_string(),
                                context: caller_context.clone(),
                            };
                            r1d_cross_body_unwind_successors.insert(next.node.clone());
                            let mut returned = snapshot.clone();
                            returned.join_heap_from(&post);
                            if join_input(&mut in_states, next.clone(), returned) {
                                worklist.insert(next);
                            }
                        }
                    }
                }
            }
            if r1d_matched_normal_return {
                continue;
            }
        }

        if is_return && r2_enabled {
            if let Ok(plans) = &r2_plans {
                for plan in matching_return_resource_plans(plans, &point.node, &point.context) {
                    let call_state = plan.entry.activation["caller_call_state"]
                        .as_str()
                        .expect("certified call");
                    for ((site, _), continuation) in &continuations {
                        if site != call_state || !continuation.initialized {
                            continue;
                        }
                        let (returned, record) =
                            transfer_return_resource(plan, &post, &continuation.caller_memory);
                        let record_key = format!(
                            "{}|{}|{}",
                            point.node,
                            call_state,
                            plan.relation["dummy_ret_state"]
                                .as_str()
                                .expect("certified return proxy")
                        );
                        r2_records.insert(record_key.clone(), record);
                        r2_pre_states.insert(
                            record_key,
                            IdentityMemoryDump::from_memory(&continuation.caller_memory),
                        );
                        let return_point = IdentityAnalysisPoint {
                            node: plan.relation["dummy_ret_state"]
                                .as_str()
                                .expect("certified proxy")
                                .into(),
                            context: continuation.caller_context.clone(),
                        };
                        r2_post_points.insert(return_point.clone());
                        if join_input(&mut in_states, return_point.clone(), returned) {
                            worklist.insert(return_point);
                        }
                    }
                }
            }
            // Root termination and diverging/unwind exits never invent a return.
            continue;
        }
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
            let same_context_nexts = drop_same_context_successors(
                nexts,
                &r1d_cross_body_unwind_successors,
            );
            for succ in same_context_nexts {
                // A matched P2 Drop unwind edge has already been propagated
                // above with the caller context and caller snapshot.  Other
                // successors from the same callee node remain live; this is
                // essential for nodes that can both return normally and unwind
                // (for example a bodyless destructor summary call).
                let succ_point = IdentityAnalysisPoint {
                    node: succ,
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

    let r1d_report = r1d_enabled.then(|| {
        let records: Vec<_> = r1d_records.values().cloned().collect();
        let required_nodes = r1d_plans
            .as_ref()
            .map(|plans| {
                plans
                    .keys()
                    .filter(|node| graph_reachable.contains(*node))
                    .cloned()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let observed_nodes = r1d_records.keys().cloned().collect::<BTreeSet<_>>();
        let required = required_nodes.len();
        let complete = r1d_plans.is_ok()
            && required_nodes == observed_nodes
            && records.iter().all(|record| {
                matches!(
                    record["status"].as_str(),
                    Some("resource_binding_available" | "no_tracked_resource")
                )
            });
        let entries: Vec<_> = r1d_entry_points
            .iter()
            .filter_map(|point| {
                in_states.get(point).map(|memory| IdentityPointDump {
                    node: point.node.clone(),
                    context: point.context.clone(),
                    memory: IdentityMemoryDump::from_memory(memory),
                })
            })
            .collect();
        let caller_pre: Vec<_> = r1d_snapshots
            .iter()
            .map(|(node, (context, memory))| IdentityPointDump {
                node: node.clone(),
                context: context.clone(),
                memory: IdentityMemoryDump::from_memory(memory),
            })
            .collect();
        serde_json::json!({
            "capability": "dependency_drop_receiver_resource_continuity_v1",
            "gate": "P3-R1D-DRB2",
            "place_relation_prerequisite": "P3-R1P-PLACE1",
            "p3_drop_receiver_complete": complete,
            "required_drop_activations": required,
            "observed_drop_activations": records.len(),
            "p2_graph_reachable_drop_activations": required,
            "required_drop_proxies": required_nodes,
            "observed_drop_proxies": observed_nodes,
            "required_scope": "all_P2_graph_reachable_mir_drop_activations",
            "deferred_drop_activations": 0,
            "deferred_drop_proxies": Vec::<String>::new(),
            "status": r1d_plans.as_ref().err(),
            "bindings": records,
            "callee_entry_states": entries,
            "caller_pre_drop_states": caller_pre,
            "semantics": "MAY_same_canonical_resource_on_P2_mir_drop_receiver_entry",
            "resource_effect_continuity": "not_implemented_R1D",
            "ownership_liveness": "not_inferred",
            "fresh_resource_creation": "forbidden",
        })
    });

    let r2_report = r2_enabled.then(|| {
        let records:Vec<_>=r2_records.values().cloned().collect();
        let required = r2_plans
            .as_ref()
            .map(|plans| {
                plans
                    .iter()
                    .filter(|(source, _)| graph_reachable.contains(*source))
                    .map(|(_, bucket)| bucket.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        let complete=r2_plans.is_ok() && required==records.len() && r1_plans.is_ok()
            && r1_records.values().flatten().all(|r|matches!(r["status"].as_str(),Some("resource_binding_available"|"no_tracked_resource")))
            && records.iter().all(|r|matches!(r["status"].as_str(),Some(
                "resource_binding_available"|"no_tracked_resource"|"heap_effects_joined_into_caller_snapshot"
            )));
        let post:Vec<_>=r2_post_points.iter().filter_map(|p|in_states.get(p).map(|m|IdentityPointDump{node:p.node.clone(),context:p.context.clone(),memory:IdentityMemoryDump::from_memory(m)})).collect();
        serde_json::json!({"capability":"dependency_resource_identity_continuity_v1","gate":"P3-R2-RET1",
            "p3_return_continuity_complete":complete,"required_normal_returns":required,"observed_normal_returns":records.len(),"status":r2_plans.err(),"return_bindings":records,
            "caller_pre_return_states":r2_pre_states,"caller_post_return_states":post,
            "semantics":"MAY_return_value_plus_global_heap_identity_store_on_P2_normal_pop; spawn_may_once callbacks restore caller locals and join callback heap effects without provider return rebasing","side_effect_continuity":"HT2_global_heap_identity_weak_join_only; spawn_may_once callback global heap effects joined; other_side_effects_not_evaluated",
            "unwind_return_continuity":"forbidden","general_unwind_resource_continuity":"deferred_to_P3_R3"})
    });
    let r1p_report = p3_place_relation_enabled().then(|| {
        let mut relations = Vec::new();
        for (point, memory) in &post_states {
            for (place, resources) in &memory.place_resources {
                relations.push(serde_json::json!({
                    "node": point.node.clone(),
                    "context": point.context.clone(),
                    "place": place.clone(),
                    "resources": resources.clone(),
                    "certainty": "may",
                    "relation_kind": "typed_place_to_canonical_resource",
                }));
            }
        }
        serde_json::json!({
            "capability": "dependency_place_resource_relations_v1",
            "gate": "P3-R1P-PLACE1",
            "status": "candidate_requires_independent_gate_verification",
            "authoritative_relation_count": relations.len(),
            "relations": relations,
            "semantics": "typed_MIR_place_resource_identity_only",
            "resource_state_effects": "not_evaluated",
            "ownership_liveness": "not_inferred",
            "heap_shape": "not_inferred",
        })
    });
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
            "semantics":"MAY_points_to_plus_global_heap_identity_store_at_represented_Rust_call_entry",
            "return_continuity":"not_evaluated_R1","side_effect_continuity":"HT2_global_heap_identity_store_propagated; other_side_effects_not_evaluated",
            "unwind_resource_continuity":"deferred_to_P3_R3"})
    });
    AllocationIdentityState {
        dependency_drop_receiver_resource_continuity_v1:r1d_report,
        dependency_resource_return_continuity_v1:r2_report,
        dependency_resource_identity_continuity_v1:r1_report,
        dependency_place_resource_relations_v1:r1p_report,
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
    fn string_from_str_may_fresh_does_not_transfer_borrowed_source_identity() {
        let source = rust("main", 1);
        let dest = rust("main", 2);
        let borrowed = alloc("borrowed-input");
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(source.clone(), borrowed.clone());
        let args = [crate::structs::MirCallArgument {
            arg: "Local(_1)".to_string(),
            is_mutable: Some(false),
        }];
        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "<std::string::String as std::convert::From<&str>>::from",
            &args,
            "Local(_2)",
            &mut mem,
        );
        let result = mem.points_to(&dest);
        assert_eq!(result.len(), 1);
        assert!(!result.contains(&borrowed), "String::from(&str) copies; it must not take ownership of the borrowed source allocation");
        let only = result.iter().next().unwrap();
        assert!(matches!(&only.site, AllocationSiteId::RustCall { callee, .. }
            if callee.contains("String as std::convert::From<&str>>::from")));
    }

    #[test]
    fn producer_certified_library_summary_drives_identity_without_callee_text() {
        let source = rust("main", 1);
        let destination = rust("main", 2);
        let allocation = alloc("producer-certified");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_fresh(source.clone(), allocation.clone());

        let evidence = RustAllocationDispositionEvidence {
            kind: crate::structs::RustAllocationDispositionEvidenceKind::BoxFromRaw,
            callee_def_path: "alloc::boxed::Box::from_raw".to_string(),
            owner_def_path: Some("alloc::boxed::Box".to_string()),
        };
        let arguments = vec![crate::structs::MirCallArgument {
            arg: "Local(_1) [mutable]".to_string(),
            is_mutable: Some(true),
        }];

        assert!(apply_producer_certified_identity_relation(
            "main",
            &arguments,
            "Local(_2)",
            &evidence,
            &mut memory,
        ));
        assert_eq!(memory.points_to(&destination), BTreeSet::from([allocation.clone()]));

        let no_pointee_effect = RustAllocationDispositionEvidence {
            kind: crate::structs::RustAllocationDispositionEvidenceKind::MemDropRawPointer,
            callee_def_path: "core::mem::drop".to_string(),
            owner_def_path: None,
        };
        assert!(!apply_producer_certified_identity_relation(
            "main",
            &arguments,
            "Local(_3)",
            &no_pointee_effect,
            &mut memory,
        ));
        assert!(memory.points_to(&rust("main", 3)).is_empty());
    }

    #[test]
    fn producer_certified_summary_resolves_exact_projected_argument_without_sibling_conflation() {
        let receiver = rust("drop_impl", 1);
        let owner = rust("drop_impl", 9);
        let destination = rust("drop_impl", 2);
        let wanted = alloc("field-0");
        let sibling = alloc("field-1");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_stack_refs(
            receiver.clone(),
            BTreeSet::from([PlaceId {
                base: owner.clone(),
                projection: Vec::new(),
            }]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: owner.clone(),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([wanted.clone()]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: owner,
                projection: vec![PlaceProjection::Field { index: 1 }],
            },
            BTreeSet::from([sibling]),
        );

        let evidence = RustAllocationDispositionEvidence {
            kind: crate::structs::RustAllocationDispositionEvidenceKind::BoxFromRaw,
            callee_def_path: "alloc::boxed::Box::from_raw".to_string(),
            owner_def_path: Some("alloc::boxed::Box".to_string()),
        };
        let arguments = vec![crate::structs::MirCallArgument {
            arg: "copy ((*_1).0: *mut String)".to_string(),
            is_mutable: Some(false),
        }];

        assert!(apply_producer_certified_identity_relation(
            "drop_impl",
            &arguments,
            "Local(_2)",
            &evidence,
            &mut memory,
        ));
        assert_eq!(memory.points_to(&destination), BTreeSet::from([wanted]));
    }

    #[test]
    fn producer_certified_summary_projected_argument_fails_closed_when_shape_is_unproved() {
        let evidence = RustAllocationDispositionEvidence {
            kind: crate::structs::RustAllocationDispositionEvidenceKind::CStringFromRaw,
            callee_def_path: "alloc::ffi::c_str::CString::from_raw".to_string(),
            owner_def_path: Some("alloc::ffi::c_str::CString".to_string()),
        };
        let indexed = vec![crate::structs::MirCallArgument {
            arg: "copy ((*_1)[_3])".to_string(),
            is_mutable: Some(false),
        }];
        let projected = vec![crate::structs::MirCallArgument {
            arg: "copy ((*_1).0: *mut i8)".to_string(),
            is_mutable: Some(false),
        }];

        let mut memory = AllocationIdentityMemory::default();
        memory.assign_fresh(rust("f", 1), alloc("carrier-not-pointee"));

        assert!(!apply_producer_certified_identity_relation(
            "f", &indexed, "Local(_2)", &evidence, &mut memory,
        ));
        assert!(memory.points_to(&rust("f", 2)).is_empty());

        // A projected field without an exact stack referent must not collapse
        // to the carrier identity merely because `_1` itself has an allocation.
        assert!(!apply_producer_certified_identity_relation(
            "f", &projected, "Local(_2)", &evidence, &mut memory,
        ));
        assert!(memory.points_to(&rust("f", 2)).is_empty());
    }

    #[test]
    fn std_nonnull_new_unchecked_preserves_pointee_identity_but_new_is_not_flattened() {
        let source = rust("main", 1);
        let destination = rust("main", 2);
        let option_destination = rust("main", 3);
        let allocation = alloc("nonnull-A");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_fresh(source, allocation.clone());
        let args = [crate::structs::MirCallArgument {
            arg: "Local(_1) [mutable]".to_string(),
            is_mutable: Some(true),
        }];

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "core::ptr::non_null::NonNull::<u8>::new_unchecked",
            &args,
            "Local(_2)",
            &mut memory,
        );
        assert_eq!(memory.points_to(&destination), BTreeSet::from([allocation]));

        // `NonNull::new` returns Option<NonNull<T>>.  Until the aggregate
        // wrapper relation is represented explicitly, flattening its pointee
        // directly onto the result local would overstate precision.
        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "core::ptr::non_null::NonNull::<u8>::new",
            &args,
            "Local(_3)",
            &mut memory,
        );
        assert!(memory.points_to(&option_destination).is_empty());
    }

    #[test]
    fn std_nonnull_view_through_exact_field_reference_preserves_only_that_resource() {
        let receiver = rust("drop_impl", 1);
        let owner = rust("drop_impl", 8);
        let destination = rust("drop_impl", 2);
        let wanted = alloc("nonnull-field");
        let sibling = alloc("sibling-field");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_stack_refs(
            receiver.clone(),
            BTreeSet::from([PlaceId {
                base: owner.clone(),
                projection: vec![PlaceProjection::Field { index: 0 }],
            }]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: owner.clone(),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([wanted.clone()]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: owner,
                projection: vec![PlaceProjection::Field { index: 1 }],
            },
            BTreeSet::from([sibling]),
        );
        let args = [crate::structs::MirCallArgument {
            arg: "Local(_1) [mutable]".to_string(),
            is_mutable: Some(true),
        }];

        transfer_library_call(
            "rust::drop_impl::bb0",
            "drop_impl",
            &[],
            "core::ptr::non_null::NonNull::<u8>::as_mut",
            &args,
            "Local(_2)",
            &mut memory,
        );
        assert_eq!(memory.points_to(&destination), BTreeSet::from([wanted]));
    }

    #[test]
    fn nonnull_lookalike_name_is_not_a_standard_library_identity_summary() {
        let source = rust("main", 1);
        let destination = rust("main", 2);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_fresh(source, alloc("A"));
        let args = [crate::structs::MirCallArgument {
            arg: "Local(_1)".to_string(),
            is_mutable: Some(false),
        }];

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "mycrate::MyNonNull::<u8>::new_unchecked",
            &args,
            "Local(_2)",
            &mut memory,
        );
        assert!(memory.points_to(&destination).is_empty());

        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "mycrate::MyNonNull::<u8>::as_ptr",
            &args,
            "Local(_2)",
            &mut memory,
        );
        assert!(memory.points_to(&destination).is_empty());
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
                atomic_ptr_evidence: None,
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
            resource_place_relation_v1: None,
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
        let at_call = result.by_node.get("rust::main::bb1").unwrap();
        let in_callee = result.by_node.get("rust::foo::bb0").unwrap();
        let at_return = result.by_node.get("rust::main::bb2").unwrap();
        let main_1 = rust("main", 1);
        let main_2 = rust("main", 2);
        let foo_1 = rust("foo", 1);

        // Actual -> formal continuity is an intra-activation property.  Check
        // it while the callee frame is live, rather than requiring the formal
        // local to leak into the caller after return.
        assert_eq!(at_call.points_to(&main_1), in_callee.points_to(&foo_1));
        assert_eq!(in_callee.points_to(&foo_1).len(), 1);

        // Return continuity preserves the same AbstractAllocId at the caller
        // destination, while the completed callee frame is no longer visible.
        assert_eq!(at_return.points_to(&main_1), at_return.points_to(&main_2));
        assert_eq!(at_return.points_to(&main_1).len(), 1);
        assert!(at_return.points_to(&foo_1).is_empty());
    }


    #[test]
    fn act_lite_standard_call_boundary_does_not_recycle_prior_callee_locals() {
        use crate::structs::{GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let call = RustCallMetadata {
            caller_function: "main".into(),
            call_node: "rust::main::bb0".into(),
            callee_function: "foo".into(),
            dummy_call_node: "dummyCall::main::bb0".into(),
            dummy_ret_node: "dummyRet::main::bb0".into(),
            arguments: vec![crate::structs::MirCallArgument {
                arg: "Local(_1)".into(),
                is_mutable: Some(false),
            }],
            return_place: "_2".into(),
            return_node: "rust::main::bb1".into(),
            is_closure: false,
        };
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: Vec::new(),
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::from([(
                "foo".into(),
                RustFunctionMetadata {
                    name: "foo".into(),
                    arg_count: 1,
                    entry_node: "rust::foo::bb0".into(),
                    return_nodes: vec!["rust::foo::bb1".into()],
                },
            )]),
            rust_calls: vec![call.clone()],
            dependency_body_ingestion_v1: None,
        };

        let argument = alloc("argument");
        let stale = alloc("prior-activation-local");
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_points_to(rust("main", 1), BTreeSet::from([argument.clone()]));

        let first_entry = bind_actuals_to_formals(&icfg, &call, &caller);
        assert_eq!(
            first_entry.points_to(&rust("foo", 1)),
            BTreeSet::from([argument.clone()])
        );
        assert!(first_entry.points_to(&rust("foo", 29)).is_empty());

        let mut first_exit = first_entry;
        first_exit.assign_points_to(rust("foo", 29), BTreeSet::from([stale]));
        let returned = bind_return_to_caller(&call, &first_exit, &caller);

        assert!(
            returned.points_to(&rust("foo", 29)).is_empty(),
            "ordinary callee locals must not escape a completed activation"
        );

        let second_entry = bind_actuals_to_formals(&icfg, &call, &returned);
        assert!(
            second_entry.points_to(&rust("foo", 29)).is_empty(),
            "a later invocation must not start with a prior activation's local identity"
        );
        assert_eq!(
            second_entry.points_to(&rust("foo", 1)),
            BTreeSet::from([argument])
        );
    }

    #[test]
    fn act_lite_standard_return_preserves_return_value_and_caller_referent_effects() {
        use crate::structs::{GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let call = RustCallMetadata {
            caller_function: "main".into(),
            call_node: "rust::main::bb0".into(),
            callee_function: "foo".into(),
            dummy_call_node: "dummyCall::main::bb0".into(),
            dummy_ret_node: "dummyRet::main::bb0".into(),
            arguments: vec![crate::structs::MirCallArgument {
                arg: "Local(_1)".into(),
                is_mutable: Some(true),
            }],
            return_place: "_2".into(),
            return_node: "rust::main::bb1".into(),
            is_closure: false,
        };
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: Vec::new(),
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::from([(
                "foo".into(),
                RustFunctionMetadata {
                    name: "foo".into(),
                    arg_count: 1,
                    entry_node: "rust::foo::bb0".into(),
                    return_nodes: vec!["rust::foo::bb1".into()],
                },
            )]),
            rust_calls: vec![call.clone()],
            dependency_body_ingestion_v1: None,
        };

        let before = alloc("before");
        let after = alloc("after");
        let returned_allocation = alloc("returned");
        let caller_place = PlaceId {
            base: rust("main", 9),
            projection: Vec::new(),
        };

        let mut caller = AllocationIdentityMemory::default();
        caller.assign_stack_refs(rust("main", 1), BTreeSet::from([caller_place.clone()]));
        caller.assign_points_to(caller_place.base.clone(), BTreeSet::from([before]));

        let mut callee = bind_actuals_to_formals(&icfg, &call, &caller);
        assert_eq!(
            callee.stack_refs(&rust("foo", 1)),
            BTreeSet::from([caller_place.clone()])
        );
        callee.assign_points_to(caller_place.base.clone(), BTreeSet::from([after.clone()]));
        callee.assign_points_to(
            rust("foo", 0),
            BTreeSet::from([returned_allocation.clone()]),
        );
        callee.assign_points_to(rust("foo", 17), BTreeSet::from([alloc("unrelated-callee")]));

        let out = bind_return_to_caller(&call, &callee, &caller);

        assert_eq!(
            out.points_to(&caller_place.base),
            BTreeSet::from([after]),
            "caller-visible effects through an actual reference must survive"
        );
        assert_eq!(
            out.points_to(&rust("main", 2)),
            BTreeSet::from([returned_allocation]),
            "the explicit return value must be rebound to the caller destination"
        );
        assert!(
            out.points_to(&rust("foo", 17)).is_empty(),
            "unrelated callee locals must not be joined into the caller"
        );
    }

    #[test]
    fn act_lite_standard_call_and_return_preserve_projected_resources() {
        use crate::structs::{GlobalICFGOrdered, RustCallMetadata, RustFunctionMetadata};

        let call = RustCallMetadata {
            caller_function: "main".into(),
            call_node: "rust::main::bb0".into(),
            callee_function: "foo".into(),
            dummy_call_node: "dummyCall::main::bb0".into(),
            dummy_ret_node: "dummyRet::main::bb0".into(),
            arguments: vec![crate::structs::MirCallArgument {
                arg: "Local(_1)".into(),
                is_mutable: Some(false),
            }],
            return_place: "_2".into(),
            return_node: "rust::main::bb1".into(),
            is_closure: false,
        };
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: Vec::new(),
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::from([(
                "foo".into(),
                RustFunctionMetadata {
                    name: "foo".into(),
                    arg_count: 1,
                    entry_node: "rust::foo::bb0".into(),
                    return_nodes: vec!["rust::foo::bb1".into()],
                },
            )]),
            rust_calls: vec![call.clone()],
            dependency_body_ingestion_v1: None,
        };

        let argument_resource = alloc("argument-field-resource");
        let return_resource = alloc("return-field-resource");
        let arg_projection = vec![PlaceProjection::Field { index: 0 }];
        let ret_projection = vec![PlaceProjection::Field { index: 1 }];

        let mut caller = AllocationIdentityMemory::default();
        caller.assign_place_resources(
            PlaceId {
                base: rust("main", 1),
                projection: arg_projection.clone(),
            },
            BTreeSet::from([argument_resource.clone()]),
        );

        let mut callee = bind_actuals_to_formals(&icfg, &call, &caller);
        assert_eq!(
            callee.resources_for_place(&PlaceId {
                base: rust("foo", 1),
                projection: arg_projection,
            }),
            BTreeSet::from([argument_resource])
        );

        callee.assign_place_resources(
            PlaceId {
                base: rust("foo", 0),
                projection: ret_projection.clone(),
            },
            BTreeSet::from([return_resource.clone()]),
        );
        let out = bind_return_to_caller(&call, &callee, &caller);
        assert_eq!(
            out.resources_for_place(&PlaceId {
                base: rust("main", 2),
                projection: ret_projection,
            }),
            BTreeSet::from([return_resource])
        );
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
                    resource_place_relation_v1: None,
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_4, &_2))".to_string(),
                    place: Some("Local(_4) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("&_2".to_string()),
                },
                MirStatement {
                    resource_place_relation_v1: None,
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_3, {closure@test.rs:1:1: 1:2} { ptr: move _4 }))".to_string(),
                    place: Some("Local(_3)".to_string()),
                    is_mutable: Some(false),
                    rvalue: Some("{closure@test.rs:1:1: 1:2} { ptr: move _4 }".to_string()),
                },
                MirStatement {
                    resource_place_relation_v1: None,
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
                atomic_ptr_evidence: None,
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
                    resource_place_relation_v1: None,
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_8, deref_copy ((*_1).0: &*mut i32)))".to_string(),
                    place: Some("Local(_8) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("deref_copy ((*_1).0: &*mut i32)".to_string()),
                },
                MirStatement {
                    resource_place_relation_v1: None,
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
                atomic_ptr_evidence: None,
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
                    resource_place_relation_v1: None,
                    source_info: source_info(),
                    kind: "Assign".to_string(),
                    details: "Assign((_9, deref_copy ((*_1).0: &*mut i32)))".to_string(),
                    place: Some("Local(_9) [mutable]".to_string()),
                    is_mutable: Some(true),
                    rvalue: Some("deref_copy ((*_1).0: &*mut i32)".to_string()),
                },
                MirStatement {
                    resource_place_relation_v1: None,
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
                atomic_ptr_evidence: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
            resource_place_relation_v1: None,
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
                resource_place_relation_v1: None,
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


      #[test]
        fn r2b_scalar_field_write_preserves_sibling_projected_identity() {
            let aggregate = rust("main", 2);
            let allocation = alloc("backing");

            let head = PlaceId {
                base: aggregate.clone(),
                projection: vec![
                    PlaceProjection::Field { index: 1 },
                ],
            };
            let written = PlaceId {
                base: aggregate.clone(),
                projection: vec![
                    PlaceProjection::Field { index: 3 },
                ],
            };

            let mut mem = AllocationIdentityMemory::default();

            mem.assign_place_points_to(
                head.clone(),
                BTreeSet::from([allocation.clone()]),
            );
            mem.assign_place_points_to(
                written.clone(),
                BTreeSet::from([alloc("stale-field3")]),
            );

            let stmt = MirStatement {
                resource_place_relation_v1: None,
                source_info: source_info(),
                kind: "Assign".to_string(),
                details:
                    "Assign(((_2.3: bool), const true))"
                        .to_string(),
                place: Some(
                    "Local(_2) [mutable] -> Field(3, Type: bool)"
                        .to_string(),
                ),
                is_mutable: Some(true),
                rvalue: Some("const true".to_string()),
            };

            transfer_statement(
                "main",
                &stmt,
                &mut mem,
            );

            assert!(
                mem.points_to_place(&written).is_empty(),
                "writing field 3 must strongly overwrite stale field-3 identity",
            );
            assert_eq!(
                mem.points_to_place(&head),
                BTreeSet::from([allocation]),
                "writing field 3 must not erase sibling field 1 identity",
            );
        }

    #[test]
    fn r2c_projected_copy_through_stack_reference_loads_exact_field_identity() {
        let reference = rust("main", 1);
        let owner = rust("main", 2);
        let result = rust("main", 7);
        let backing = alloc("backing-M");
        let sibling = alloc("sibling");

        let owner_place = PlaceId {
            base: owner.clone(),
            projection: Vec::new(),
        };
        let field1 = PlaceId {
            base: owner.clone(),
            projection: vec![PlaceProjection::Field { index: 1 }],
        };
        let field2 = PlaceId {
            base: owner,
            projection: vec![PlaceProjection::Field { index: 2 }],
        };

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_stack_refs(reference, BTreeSet::from([owner_place]));
        mem.assign_place_points_to(field1, BTreeSet::from([backing.clone()]));
        mem.assign_place_points_to(field2, BTreeSet::from([sibling]));

        let stmt = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_7, copy ((*_1).1: *mut std::option::Option<u64>)))".into(),
            place: Some("Local(_7) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("copy ((*_1).1: *mut std::option::Option<u64>)".into()),
        };

        transfer_statement("main", &stmt, &mut mem);

        assert_eq!(mem.points_to(&result), BTreeSet::from([backing]));
        assert!(mem.stack_refs(&result).is_empty());
    }

    #[test]
    fn r2c_projected_copy_preserves_exact_projected_stack_reference() {
        let reference = rust("main", 1);
        let owner = rust("main", 2);
        let result = rust("main", 7);
        let target = PlaceId {
            base: rust("main", 9),
            projection: vec![PlaceProjection::Field { index: 0 }],
        };

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_stack_refs(
            reference,
            BTreeSet::from([PlaceId {
                base: owner.clone(),
                projection: Vec::new(),
            }]),
        );
        mem.assign_place_stack_refs(
            PlaceId {
                base: owner,
                projection: vec![PlaceProjection::Field { index: 1 }],
            },
            BTreeSet::from([target.clone()]),
        );

        let stmt = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_7, copy ((*_1).1: &mut u64)))".into(),
            place: Some("Local(_7) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("copy ((*_1).1: &mut u64)".into()),
        };

        transfer_statement("main", &stmt, &mut mem);
        assert_eq!(mem.stack_refs(&result), BTreeSet::from([target]));
        assert!(mem.points_to(&result).is_empty());
    }

    #[test]
    fn r2c_projected_copy_rebases_below_projected_stack_referent() {
        let reference = rust("main", 1);
        let owner = rust("main", 2);
        let result = rust("main", 8);
        let backing = alloc("nested-M");

        let referenced_prefix = PlaceId {
            base: owner.clone(),
            projection: vec![PlaceProjection::Field { index: 0 }],
        };
        let nested = PlaceId {
            base: owner,
            projection: vec![
                PlaceProjection::Field { index: 0 },
                PlaceProjection::Field { index: 2 },
            ],
        };

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_stack_refs(reference, BTreeSet::from([referenced_prefix]));
        mem.assign_place_points_to(nested, BTreeSet::from([backing.clone()]));

        let stmt = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_8, copy ((*_1).2: *mut u8)))".into(),
            place: Some("Local(_8) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("copy ((*_1).2: *mut u8)".into()),
        };

        transfer_statement("main", &stmt, &mut mem);
        assert_eq!(mem.points_to(&result), BTreeSet::from([backing]));
    }

    #[test]
    fn r2c_projected_heap_shape_without_stack_place_fails_closed() {
        let reference = rust("main", 1);
        let result = rust("main", 7);
        let heap = alloc("heap-owner");

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(reference, heap);

        let stmt = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_7, copy ((*_1).1: *mut u8)))".into(),
            place: Some("Local(_7) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("copy ((*_1).1: *mut u8)".into()),
        };

        transfer_statement("main", &stmt, &mut mem);
        assert!(mem.event_allocations(&result).is_empty());
    }

    #[test]
    fn r2c_unsupported_deref_projection_does_not_copy_reference_carrier_identity() {
        let reference = rust("main", 1);
        let result = rust("main", 7);
        let carrier = alloc("reference-carrier");

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_fresh(reference, carrier);

        let stmt = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_7, copy ((*_1)[_3])))".into(),
            place: Some("Local(_7) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("copy ((*_1)[_3])".into()),
        };

        transfer_statement("main", &stmt, &mut mem);
        assert!(
            mem.event_allocations(&result).is_empty(),
            "unsupported dereference projection must fail closed rather than copy the carrier",
        );
    }

    #[test]
    fn r2c_projected_load_flows_through_add_and_cast_as_access_base() {
        let reference = rust("main", 1);
        let owner = rust("main", 2);
        let loaded = rust("main", 8);
        let offset = rust("main", 7);
        let cast = rust("main", 6);
        let backing = alloc("backing-M");

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_stack_refs(
            reference,
            BTreeSet::from([PlaceId {
                base: owner.clone(),
                projection: Vec::new(),
            }]),
        );
        mem.assign_place_points_to(
            PlaceId {
                base: owner,
                projection: vec![PlaceProjection::Field { index: 1 }],
            },
            BTreeSet::from([backing.clone()]),
        );

        let load = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_8, copy ((*_1).1: *mut std::option::Option<u64>)))".into(),
            place: Some("Local(_8) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("copy ((*_1).1: *mut std::option::Option<u64>)".into()),
        };
        transfer_statement("main", &load, &mut mem);
        assert_eq!(mem.points_to(&loaded), BTreeSet::from([backing.clone()]));

        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "std::ptr::mut_ptr::<impl *mut u8>::add",
            &[
                crate::structs::MirCallArgument {
                    arg: "Local(_8) [mutable]".into(),
                    is_mutable: Some(true),
                },
                crate::structs::MirCallArgument {
                    arg: "const 1_usize".into(),
                    is_mutable: None,
                },
            ],
            "_7",
            &mut mem,
        );

        assert!(mem.points_to(&offset).is_empty());
        assert_eq!(
            mem.access_bases.get(&offset).cloned().unwrap_or_default(),
            BTreeSet::from([backing.clone()]),
        );

        let cast_stmt = MirStatement {
            resource_place_relation_v1: None,
            source_info: source_info(),
            kind: "Assign".into(),
            details: "Assign((_6, move _7 as *const u8 (PtrToPtr)))".into(),
            place: Some("Local(_6) [mutable]".into()),
            is_mutable: Some(true),
            rvalue: Some("move _7 as *const u8 (PtrToPtr)".into()),
        };
        transfer_statement("main", &cast_stmt, &mut mem);

        assert!(mem.points_to(&cast).is_empty());
        assert_eq!(
            mem.access_bases.get(&cast).cloned().unwrap_or_default(),
            BTreeSet::from([backing]),
        );
    }

    fn ht2_arg(local: u32) -> crate::structs::MirCallArgument {
        crate::structs::MirCallArgument {
            arg: format!("Local(_{local}) [mutable]"),
            is_mutable: Some(true),
        }
    }

    #[test]
    fn ht2_ptr_write_then_read_preserves_projected_resource_identity() {
        let backing = alloc("backing-M");
        let payload = alloc("payload-R");
        let pointer = rust("main", 1);
        let value = rust("main", 2);
        let result = rust("main", 5);

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_access_bases(pointer.clone(), BTreeSet::from([backing.clone()]));
        mem.assign_place_points_to(
            PlaceId {
                base: value.clone(),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([payload.clone()]),
        );

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(2)],
            "_90",
            &mut mem,
        );

        let stored = mem
            .heap_values
            .get(&backing)
            .expect("ptr::write must materialize a heap summary");
        assert_eq!(
            stored
                .points_to
                .get(&vec![PlaceProjection::Field { index: 0 }])
                .cloned()
                .unwrap_or_default(),
            BTreeSet::from([payload.clone()]),
        );

        let heap_before_read = mem.heap_values.clone();
        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "std::ptr::read",
            &[ht2_arg(1)],
            "_5",
            &mut mem,
        );

        assert_eq!(
            mem.points_to_place(&PlaceId {
                base: result,
                projection: vec![PlaceProjection::Field { index: 0 }],
            }),
            BTreeSet::from([payload]),
        );
        assert_eq!(
            mem.heap_values, heap_before_read,
            "ptr::read must be non-destructive",
        );
    }

    #[test]
    fn ht2_empty_write_cannot_strong_kill_previous_may_heap_identity() {
        let backing = alloc("backing-M");
        let payload = alloc("payload-R");
        let pointer = rust("main", 1);
        let some_value = rust("main", 2);

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_access_bases(pointer, BTreeSet::from([backing.clone()]));
        mem.assign_place_points_to(
            PlaceId {
                base: some_value,
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([payload.clone()]),
        );

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(2)],
            "_90",
            &mut mem,
        );
        let before = mem.heap_values.clone();

        // Local _3 intentionally carries no identity, modeling None/scalar
        // payload from the identity domain's perspective.
        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(3)],
            "_91",
            &mut mem,
        );

        assert_eq!(
            mem.heap_values, before,
            "without must-slot evidence an empty write must not erase MAY contents",
        );
        assert_eq!(
            mem.heap_values[&backing].points_to
                [&vec![PlaceProjection::Field { index: 0 }]],
            BTreeSet::from([payload]),
        );
    }

    #[test]
    fn ht2_repeated_writes_weakly_join_same_abstract_base() {
        let backing = alloc("backing-M");
        let a = alloc("payload-A");
        let b = alloc("payload-B");

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_access_bases(rust("main", 1), BTreeSet::from([backing.clone()]));
        mem.assign_place_points_to(
            PlaceId {
                base: rust("main", 2),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([a.clone()]),
        );
        mem.assign_place_points_to(
            PlaceId {
                base: rust("main", 3),
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([b.clone()]),
        );

        for (node, source) in [("bb0", 2), ("bb1", 3)] {
            transfer_library_call(
                &format!("rust::main::{node}"),
                "main",
                &[],
                "std::ptr::write",
                &[ht2_arg(1), ht2_arg(source)],
                "_90",
                &mut mem,
            );
        }

        assert_eq!(
            mem.heap_values[&backing].points_to
                [&vec![PlaceProjection::Field { index: 0 }]],
            BTreeSet::from([a, b]),
        );
    }

    #[test]
    fn ht2_distinct_allocation_bases_do_not_cross_contaminate() {
        let m1 = alloc("M1");
        let m2 = alloc("M2");
        let payload = alloc("R");

        let mut mem = AllocationIdentityMemory::default();
        mem.assign_access_bases(rust("main", 1), BTreeSet::from([m1.clone()]));
        mem.assign_access_bases(rust("main", 2), BTreeSet::from([m2]));
        mem.assign_points_to(rust("main", 3), BTreeSet::from([payload]));

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(3)],
            "_90",
            &mut mem,
        );
        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "std::ptr::read",
            &[ht2_arg(2)],
            "_4",
            &mut mem,
        );

        assert!(mem.event_allocations(&rust("main", 4)).is_empty());
        assert!(mem.heap_values.contains_key(&m1));
    }

    #[test]
    fn ht2_roundtrip_preserves_access_only_pointer_provenance() {
        let storage = alloc("storage-M");
        let pointee = alloc("interior-base-R");
        let mut mem = AllocationIdentityMemory::default();

        mem.assign_access_bases(rust("main", 1), BTreeSet::from([storage]));
        mem.assign_access_bases(rust("main", 2), BTreeSet::from([pointee.clone()]));

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(2)],
            "_90",
            &mut mem,
        );
        transfer_library_call(
            "rust::main::bb1",
            "main",
            &[],
            "std::ptr::read",
            &[ht2_arg(1)],
            "_3",
            &mut mem,
        );

        assert!(mem.points_to(&rust("main", 3)).is_empty());
        assert_eq!(
            mem.access_bases.get(&rust("main", 3)).cloned().unwrap_or_default(),
            BTreeSet::from([pointee]),
        );
    }

    #[test]
    fn ht2_projected_pointer_argument_fails_closed() {
        let backing = alloc("M");
        let payload = alloc("R");
        let mut mem = AllocationIdentityMemory::default();

        mem.assign_access_bases(rust("main", 1), BTreeSet::from([backing]));
        mem.assign_points_to(rust("main", 2), BTreeSet::from([payload]));

        let projected_pointer = crate::structs::MirCallArgument {
            arg: "copy ((*_1).0: *mut u8)".into(),
            is_mutable: Some(false),
        };

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[projected_pointer, ht2_arg(2)],
            "_90",
            &mut mem,
        );

        assert!(
            mem.heap_values.is_empty(),
            "HT2 must not collapse an unsupported projected pointer argument to its carrier",
        );
    }

    #[test]
    fn ht2_ptr_read_projected_destination_does_not_overwrite_base_local() {
        let backing = alloc("M");
        let payload = alloc("R");
        let stale = alloc("base-stale");
        let mut mem = AllocationIdentityMemory::default();

        mem.assign_access_bases(rust("main", 1), BTreeSet::from([backing.clone()]));
        mem.assign_points_to(rust("main", 5), BTreeSet::from([stale.clone()]));
        mem.heap_values.insert(
            backing,
            HeapIdentityValue {
                points_to: BTreeMap::from([(Vec::new(), BTreeSet::from([payload]))]),
                ..HeapIdentityValue::default()
            },
        );

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::read",
            &[ht2_arg(1)],
            "Local(_5) [mutable] -> Field(0, Type: *mut u8)",
            &mut mem,
        );

        assert_eq!(
            mem.points_to(&rust("main", 5)),
            BTreeSet::from([stale]),
            "unsupported projected read destination must fail closed rather than overwrite its base",
        );
    }

    #[test]
    fn ht2_write_ignores_unsupported_nested_heap_projection() {
        let backing = alloc("M");
        let nested = alloc("nested-R");
        let mut mem = AllocationIdentityMemory::default();

        mem.assign_access_bases(rust("main", 1), BTreeSet::from([backing]));
        mem.assign_place_points_to(
            PlaceId {
                base: rust("main", 2),
                projection: vec![
                    PlaceProjection::Deref,
                    PlaceProjection::Field { index: 0 },
                ],
            },
            BTreeSet::from([nested]),
        );

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(2)],
            "_90",
            &mut mem,
        );

        assert!(
            mem.heap_values.is_empty(),
            "HT2-R1 stores only root/Field value identity and must not invent nested heap shape",
        );
    }

    #[test]
    fn ht2_heap_join_and_order_are_pointwise_may() {
        let backing = alloc("M");
        let a = alloc("A");
        let b = alloc("B");

        let mut left = AllocationIdentityMemory::default();
        left.heap_values.insert(
            backing.clone(),
            HeapIdentityValue {
                points_to: BTreeMap::from([(
                    vec![PlaceProjection::Field { index: 0 }],
                    BTreeSet::from([a.clone()]),
                )]),
                ..HeapIdentityValue::default()
            },
        );

        let mut right = AllocationIdentityMemory::default();
        right.heap_values.insert(
            backing.clone(),
            HeapIdentityValue {
                points_to: BTreeMap::from([(
                    vec![PlaceProjection::Field { index: 0 }],
                    BTreeSet::from([b.clone()]),
                )]),
                ..HeapIdentityValue::default()
            },
        );

        let joined = left.join(&right);
        assert!(left.leq(&joined));
        assert!(right.leq(&joined));
        assert_eq!(
            joined.heap_values[&backing].points_to
                [&vec![PlaceProjection::Field { index: 0 }]],
            BTreeSet::from([a, b]),
        );
    }

    #[test]
    fn ht2_identity_dump_exposes_heap_summary_without_changing_old_fields() {
        let backing = alloc("M");
        let payload = alloc("R");
        let mut mem = AllocationIdentityMemory::default();
        mem.assign_access_bases(rust("main", 1), BTreeSet::from([backing]));
        mem.assign_points_to(rust("main", 2), BTreeSet::from([payload]));

        transfer_library_call(
            "rust::main::bb0",
            "main",
            &[],
            "std::ptr::write",
            &[ht2_arg(1), ht2_arg(2)],
            "_90",
            &mut mem,
        );

        let dump = serde_json::to_value(IdentityMemoryDump::from_memory(&mem))
            .expect("identity dump must serialize");
        assert_eq!(dump["heap_values"].as_array().map(Vec::len), Some(1));
        assert!(dump.get("points_to").is_some());
        assert!(dump.get("place_points_to").is_some());
    }


    // Nested contexts collapse at the wrapper's shared inner callsite (existing k=1).
    // A direct two-callsite test cannot expose the missing return dependency.
    fn continuation_repair_fixture(reverse: bool, third: bool) -> GlobalICFGOrdered {
        let labels: Vec<_> = if third {
            vec!["bb1", "bb2", "bb3"]
        } else if reverse {
            vec!["bb2", "bb1"]
        } else {
            vec!["bb1", "bb2"]
        };
        let mut graph = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![(
                "rust::main::bb0".into(),
                GlobalICFGNode::Mir(MirBasicBlock {
                    block_id: 0,
                    statements: vec![],
                    terminator: Some(MirTerminator::Goto {
                        details: String::new(),
                        source_info: String::new(),
                        target: labels[0].into(),
                    }),
                }),
            )],
            icfg_edges: vec![edge(
                "rust::main::bb0",
                &format!("rust::main::{}", labels[0]),
            )],
            rust_functions: BTreeMap::from([
                (
                    "main".into(),
                    crate::structs::RustFunctionMetadata {
                        name: "main".into(),
                        arg_count: 0,
                        entry_node: "rust::main::bb0".into(),
                        return_nodes: vec!["rust::main::bb9".into()],
                    },
                ),
                (
                    "wrapper".into(),
                    crate::structs::RustFunctionMetadata {
                        name: "wrapper".into(),
                        arg_count: 0,
                        entry_node: "rust::wrapper::bb0".into(),
                        return_nodes: vec!["rust::wrapper::bb1".into()],
                    },
                ),
                (
                    "leaf".into(),
                    crate::structs::RustFunctionMetadata {
                        name: "leaf".into(),
                        arg_count: 0,
                        entry_node: "rust::leaf::bb0".into(),
                        return_nodes: vec!["rust::leaf::bb1".into()],
                    },
                ),
            ]),
            rust_calls: vec![],
            dependency_body_ingestion_v1: None,
        };
        for (index, label) in labels.iter().enumerate() {
            let node = format!("rust::main::{label}");
            let next = if index + 1 < labels.len() {
                format!("rust::main::{}", labels[index + 1])
            } else {
                "rust::main::bb8".into()
            };
            let dc = format!("dummyCall::main::{label}");
            let dr = format!("dummyRet::main::{label}");
            graph.ordered_nodes.push((
                node.clone(),
                GlobalICFGNode::Mir(call_block(
                    "wrapper",
                    vec![],
                    &format!("_{}", index + 1),
                    next.rsplit("::").next().unwrap(),
                )),
            ));
            graph
                .ordered_nodes
                .push(internal_dummy_call(&dc, &node, "rust::wrapper::bb0"));
            graph
                .ordered_nodes
                .push(internal_dummy_ret(&dr, "wrapper", &next));
            graph.icfg_edges.extend([
                edge(&node, &dc),
                edge(&dc, "rust::wrapper::bb0"),
                edge("rust::wrapper::bb1", &dr),
                edge(&dr, &next),
            ]);
            graph.rust_calls.push(RustCallMetadata {
                caller_function: "main".into(),
                call_node: node,
                callee_function: "wrapper".into(),
                dummy_call_node: dc,
                dummy_ret_node: dr,
                arguments: vec![],
                return_place: format!("_{}", index + 1),
                return_node: next,
                is_closure: false,
            });
        }
        graph.ordered_nodes.extend([
            (
                "rust::wrapper::bb0".into(),
                GlobalICFGNode::Mir(call_block("leaf", vec![], "_1", "bb1")),
            ),
            internal_dummy_call(
                "dummyCall::wrapper::bb0",
                "rust::wrapper::bb0",
                "rust::leaf::bb0",
            ),
            internal_dummy_ret("dummyRet::wrapper::bb0", "leaf", "rust::wrapper::bb1"),
            (
                "rust::wrapper::bb1".into(),
                GlobalICFGNode::Mir(return_block(1, vec![copy_statement(0, 1)])),
            ),
            (
                "rust::leaf::bb0".into(),
                GlobalICFGNode::Mir(call_block(
                    "std::boxed::Box::<i32>::new",
                    vec![],
                    "_1",
                    "bb1",
                )),
            ),
            (
                "rust::leaf::bb1".into(),
                GlobalICFGNode::Mir(return_block(1, vec![copy_statement(0, 1)])),
            ),
            (
                "rust::main::bb8".into(),
                GlobalICFGNode::Mir(call_block(
                    "std::boxed::Box::<i32>::new",
                    vec![],
                    "_8",
                    "bb9",
                )),
            ),
            (
                "rust::main::bb9".into(),
                GlobalICFGNode::Mir(return_block(9, vec![])),
            ),
        ]);
        graph.icfg_edges.extend([
            edge("rust::wrapper::bb0", "dummyCall::wrapper::bb0"),
            edge("dummyCall::wrapper::bb0", "rust::leaf::bb0"),
            edge("rust::leaf::bb0", "rust::leaf::bb1"),
            edge("rust::leaf::bb1", "dummyRet::wrapper::bb0"),
            edge("dummyRet::wrapper::bb0", "rust::wrapper::bb1"),
            edge("rust::main::bb8", "rust::main::bb9"),
        ]);
        graph.rust_calls.push(RustCallMetadata {
            caller_function: "wrapper".into(),
            call_node: "rust::wrapper::bb0".into(),
            callee_function: "leaf".into(),
            dummy_call_node: "dummyCall::wrapper::bb0".into(),
            dummy_ret_node: "dummyRet::wrapper::bb0".into(),
            arguments: vec![],
            return_place: "_1".into(),
            return_node: "rust::wrapper::bb1".into(),
            is_closure: false,
        });
        graph
    }

    fn assert_continuation_repair_fixture(reverse: bool, third: bool) {
        let graph = continuation_repair_fixture(reverse, third);
        let result = fixed_point_identity_analysis(&graph, "rust::main::bb0");
        assert!(
            result.by_node.contains_key("rust::main::bb8"),
            "late continuation must receive cached normal return"
        );
        let ids = result.by_node["rust::main::bb8"].points_to(&rust("main", 8));
        assert_eq!(
            ids.len(),
            1,
            "allocation after the second call has genuine site provenance"
        );
        for call in graph
            .rust_calls
            .iter()
            .filter(|call| call.caller_function == "main")
        {
            let point = IdentityAnalysisPoint {
                node: "rust::wrapper::bb1".into(),
                context: bounded_context(&call.call_node),
            };
            assert!(
                result.by_point.contains_key(&point),
                "wrapper return for {}",
                call.call_node
            );
        }
        assert_eq!(
            serde_json::to_value(result.to_dump()).unwrap(),
            serde_json::to_value(fixed_point_identity_analysis(&graph, "rust::main::bb0").to_dump())
                .unwrap()
        );
        assert!(!result.by_point.contains_key(&IdentityAnalysisPoint {
            node: "rust::wrapper::bb1".into(),
            context: vec!["unreachable-callsite".into()]
        }));
    }
    #[test]
    fn identity_continuation_repair_nested_equivalent_inputs() {
        assert_continuation_repair_fixture(false, false);
    }
    #[test]
    fn identity_continuation_repair_reverse_callsite_order() {
        assert_continuation_repair_fixture(true, false);
    }
    #[test]
    fn identity_continuation_repair_three_subscribers() {
        assert_continuation_repair_fixture(false, true);
    }
    #[test]
    fn identity_continuation_repair_metadata_and_node_order() {
        let a = continuation_repair_fixture(false, true);
        let mut b = a.clone();
        b.rust_calls.reverse();
        b.ordered_nodes.reverse();
        b.icfg_edges.reverse();
        assert_eq!(
            serde_json::to_value(fixed_point_identity_analysis(&a, "rust::main::bb0").to_dump())
                .unwrap(),
            serde_json::to_value(fixed_point_identity_analysis(&b, "rust::main::bb0").to_dump())
                .unwrap()
        );
    }
    #[test]
    fn identity_continuation_repair_snapshot_change_and_idempotence() {
        let mut k = IdentityContinuation::default();
        let a = AllocationIdentityMemory::default();
        assert!(k.join_snapshot(&[], &a));
        assert!(!k.join_snapshot(&[], &a));
        let mut b = a.clone();
        b.assign_fresh(rust("main", 7), alloc("later"));
        assert!(k.join_snapshot(&[], &b));
        assert!(a.leq(&k.caller_memory));
        assert!(b.leq(&k.caller_memory));
        assert!(!k.join_snapshot(&[], &b));
    }
    #[test]
    fn identity_continuation_repair_materialized_only_and_deduplicated() {
        let graph = continuation_repair_fixture(false, false);
        let mut w = BTreeSet::new();
        let mut posts = BTreeMap::new();
        notify_materialized_identity_returns(&graph, "rust::wrapper::bb0", &posts, &mut w);
        assert!(w.is_empty());
        let point = IdentityAnalysisPoint {
            node: "rust::leaf::bb1".into(),
            context: bounded_context("rust::wrapper::bb0"),
        };
        posts.insert(point.clone(), AllocationIdentityMemory::default());
        // Another context, though materialized, is not a reader of this callsite.
        posts.insert(
            IdentityAnalysisPoint {
                node: point.node.clone(),
                context: bounded_context("different-site"),
            },
            AllocationIdentityMemory::default(),
        );
        notify_materialized_identity_returns(&graph, "rust::wrapper::bb0", &posts, &mut w);
        notify_materialized_identity_returns(&graph, "rust::wrapper::bb0", &posts, &mut w);
        assert_eq!(w, BTreeSet::from([point]));
    }
    #[test]
    fn identity_continuation_repair_changed_input_joins_conservatively() {
        let point = IdentityAnalysisPoint {
            node: "rust::leaf::bb0".into(),
            context: bounded_context("rust::wrapper::bb0"),
        };
        let mut states = BTreeMap::new();
        let mut a = AllocationIdentityMemory::default();
        a.assign_fresh(rust("leaf", 1), alloc("A"));
        let mut b = AllocationIdentityMemory::default();
        b.assign_fresh(rust("leaf", 1), alloc("B"));
        assert!(join_input(&mut states, point.clone(), a.clone()));
        assert!(join_input(&mut states, point.clone(), b.clone()));
        assert!(a.leq(&states[&point]));
        assert!(b.leq(&states[&point]));
        assert_eq!(states[&point].points_to(&rust("leaf", 1)).len(), 2);
        assert!(!join_input(&mut states, point, a));
    }
    fn recursive_continuation_repair_fixture(mutual: bool) -> GlobalICFGOrdered {
        let mut g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![],
            icfg_edges: vec![],
            rust_functions: BTreeMap::new(),
            rust_calls: vec![],
            dependency_body_ingestion_v1: None,
        };
        let names = if mutual {
            vec!["rec_a", "rec_b"]
        } else {
            vec!["rec_a"]
        };
        let mut calls = vec![("main", "bb0", "rec_a", "bb1")];
        for name in &names {
            let callee = if mutual && *name == "rec_a" {
                "rec_b"
            } else {
                "rec_a"
            };
            calls.push((name, "bb1", callee, "bb3"));
            g.rust_functions.insert(
                (*name).into(),
                crate::structs::RustFunctionMetadata {
                    name: (*name).into(),
                    arg_count: 0,
                    entry_node: format!("rust::{name}::bb0"),
                    return_nodes: vec![format!("rust::{name}::bb2"), format!("rust::{name}::bb3")],
                },
            );
            g.ordered_nodes.push((
                format!("rust::{name}::bb0"),
                GlobalICFGNode::Mir(MirBasicBlock {
                    block_id: 0,
                    statements: vec![],
                    terminator: Some(MirTerminator::SwitchInt {
                        details: String::new(),
                        source_info: String::new(),
                        targets: vec!["bb1".into()],
                        discr: "_1".into(),
                        otherwise: Some("bb2".into()),
                    }),
                }),
            ));
            g.ordered_nodes.push((
                format!("rust::{name}::bb2"),
                GlobalICFGNode::Mir(return_block(2, vec![])),
            ));
            g.ordered_nodes.push((
                format!("rust::{name}::bb3"),
                GlobalICFGNode::Mir(return_block(3, vec![])),
            ));
            g.icfg_edges.extend([
                edge(&format!("rust::{name}::bb0"), &format!("rust::{name}::bb1")),
                edge(&format!("rust::{name}::bb0"), &format!("rust::{name}::bb2")),
            ]);
        }
        g.rust_functions.insert(
            "main".into(),
            crate::structs::RustFunctionMetadata {
                name: "main".into(),
                arg_count: 0,
                entry_node: "rust::main::bb0".into(),
                return_nodes: vec!["rust::main::bb1".into()],
            },
        );
        g.ordered_nodes.push((
            "rust::main::bb1".into(),
            GlobalICFGNode::Mir(return_block(1, vec![])),
        ));
        for (caller, bb, callee, next) in calls {
            let site = format!("rust::{caller}::{bb}");
            let entry = format!("rust::{callee}::bb0");
            let cont = format!("rust::{caller}::{next}");
            let dc = format!("dummyCall::{caller}::{bb}");
            let dr = format!("dummyRet::{caller}::{bb}");
            g.ordered_nodes.extend([
                (
                    site.clone(),
                    GlobalICFGNode::Mir(call_block(callee, vec![], "_0", next)),
                ),
                internal_dummy_call(&dc, &site, &entry),
                internal_dummy_ret(&dr, callee, &cont),
            ]);
            g.icfg_edges
                .extend([edge(&site, &dc), edge(&dc, &entry), edge(&dr, &cont)]);
            for ret in &g.rust_functions[callee].return_nodes {
                g.icfg_edges.push(edge(ret, &dr));
            }
            g.rust_calls.push(RustCallMetadata {
                caller_function: caller.into(),
                call_node: site,
                callee_function: callee.into(),
                dummy_call_node: dc,
                dummy_ret_node: dr,
                arguments: vec![],
                return_place: "_0".into(),
                return_node: cont,
                is_closure: false,
            });
        }
        g
    }
    #[test]
    fn identity_continuation_repair_direct_recursion() {
        let g = recursive_continuation_repair_fixture(false);
        let result = fixed_point_identity_analysis(&g, "rust::main::bb0");
        assert!(result.by_node.contains_key("rust::main::bb1"));
        assert!(result.by_point.len() <= g.ordered_nodes.len() * (g.rust_calls.len() + 1));
    }
    #[test]
    fn identity_continuation_repair_mutual_recursion() {
        let g = recursive_continuation_repair_fixture(true);
        let result = fixed_point_identity_analysis(&g, "rust::main::bb0");
        assert!(result.by_node.contains_key("rust::main::bb1"));
        assert!(result.by_point.len() <= g.ordered_nodes.len() * (g.rust_calls.len() + 1));
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

    #[test]
    fn r1_aggregate_actual_with_existing_resource_is_bound_without_environment_copy() {
        let mut caller = AllocationIdentityMemory::default();
        let resource = alloc("string-buffer");
        caller.assign_fresh(var("caller", 4), resource.clone());
        caller.assign_fresh(var("caller", 8), alloc("unrelated"));
        let aggregate = json!({
            "operand":"move",
            "actual_local":4,
            "direct_local":true,
            "value_shape":"aggregate"
        });
        let (entry, records) = transfer_actual_formal(&plan(vec![aggregate]), &caller);
        assert_eq!(entry.points_to(&var("callee", 1)), BTreeSet::from([resource.clone()]));
        assert!(entry.points_to(&var("caller", 8)).is_empty());
        assert_eq!(records[0]["status"], "resource_binding_available");
        assert_eq!(records[0]["resources"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn r1_aggregate_projected_identity_is_rebased_to_formal() {
        let mut caller = AllocationIdentityMemory::default();
        let resource = alloc("field-resource");
        caller.assign_place_points_to(
            PlaceId { base: var("caller", 4), projection: vec![PlaceProjection::Field { index: 0 }] },
            BTreeSet::from([resource.clone()]),
        );
        let aggregate = json!({
            "operand":"move",
            "actual_local":4,
            "direct_local":true,
            "value_shape":"aggregate"
        });
        let (entry, records) = transfer_actual_formal(&plan(vec![aggregate]), &caller);
        assert_eq!(
            entry.points_to_place(&PlaceId { base: var("callee", 1), projection: vec![PlaceProjection::Field { index: 0 }] }),
            BTreeSet::from([resource])
        );
        assert_eq!(records[0]["status"], "resource_binding_available");
    }

    #[test]
    fn p3_r1_spawn_callback_binds_only_the_certified_closure_environment() {
        let mut caller=AllocationIdentityMemory::default();
        let captured=alloc("captured-resource");
        caller.assign_place_points_to(
            PlaceId {base:var("caller",4),projection:vec![PlaceProjection::Field{index:0}]},
            BTreeSet::from([captured.clone()]),
        );
        caller.assign_fresh(var("caller",9),alloc("unrelated"));
        let actual=json!({
            "operand":"move","actual_local":4,"direct_local":true,
            "value_shape":"closure_environment"
        });
        let mut callback=plan(vec![actual]);
        callback.activation["activation_kind"]=json!("higher_order_callback");
        callback.activation["callback_semantics"]=json!("spawn_may_once");
        callback.abi="rustc_closure_body_v1".into();
        let (entry,records)=transfer_actual_formal(&callback,&caller);
        assert_eq!(
            entry.points_to_place(&PlaceId {base:var("callee",1),projection:vec![PlaceProjection::Field{index:0}]}),
            BTreeSet::from([captured]),
        );
        assert!(entry.points_to(&var("caller",9)).is_empty());
        assert_eq!(records[0]["status"],"resource_binding_available");
    }

    #[test]
    fn ht2_r1_actual_formal_preserves_global_heap_store_without_local_environment_copy() {
        let backing = alloc("heap-M");
        let payload = alloc("heap-R");
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller",1),alloc("arg"));
        caller.assign_fresh(var("caller",8),alloc("unrelated-local"));
        caller.heap_values.insert(
            backing.clone(),
            HeapIdentityValue {
                points_to: BTreeMap::from([(
                    vec![PlaceProjection::Field { index: 0 }],
                    BTreeSet::from([payload.clone()]),
                )]),
                ..HeapIdentityValue::default()
            },
        );

        let (entry, _) = transfer_actual_formal(&plan(vec![pointer(1)]), &caller);

        assert_eq!(entry.heap_values, caller.heap_values);
        assert!(entry.points_to(&var("caller",8)).is_empty());
        assert_eq!(
            entry.heap_values[&backing].points_to
                [&vec![PlaceProjection::Field { index: 0 }]],
            BTreeSet::from([payload]),
        );
    }
}

#[cfg(test)]
mod p3_r2_tests {
    use super::*;
    use serde_json::json;
    use crate::structs::AllocationSiteId;
    fn var(f: &str,l:u32)->ProgramVarId {ProgramVarId::Rust{function:f.into(),local:l}}
    fn resource(s:&str)->AbstractAllocId {AbstractAllocId::new(AllocationSiteId::Synthetic{scope:"existing".into(),label:s.into()},vec!["existing_context".into()])}
    fn plan(shape:&str)->ReturnResourcePlan {ReturnResourcePlan{
        entry:ActualFormalPlan{activation:json!({"call_key":"K","caller_instance_id":"I0","callee_instance_id":"I1","parent_context":["outer"],"callee_context":["outer","K"],"caller_call_state":"call"}),caller_scope:"caller".into(),callee_scope:"callee".into(),abi:"Rust".into(),formals:vec![],actuals:vec![]},
        relation:json!({"callee_return_state":"return","dummy_ret_state":"proxy","continuation_state":"continuation"}),destination:Some(PlaceId{base:var("caller",3),projection:vec![]}),return_shape:shape.into()}}
    #[test]
    fn p3_r2_recursive_k1_shared_return_state_selects_only_matching_call_state() {
        let mut plans = ReturnResourcePlans::new();
        let mut left = plan("raw_pointer");
        left.entry.activation["caller_call_state"] = json!("call-A");
        left.relation["dummy_ret_state"] = json!("proxy-A");
        let mut right = plan("raw_pointer");
        right.entry.activation["caller_call_state"] = json!("call-B");
        right.relation["dummy_ret_state"] = json!("proxy-B");

        insert_return_resource_plan(&mut plans, "shared-return", left).unwrap();
        insert_return_resource_plan(&mut plans, "shared-return", right).unwrap();
        assert_eq!(plans["shared-return"].len(), 2);

        let a_context = vec!["call-A".to_string()];
        let a = matching_return_resource_plans(&plans, "shared-return", &a_context);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].relation["dummy_ret_state"], "proxy-A");

        let b_context = vec!["call-B".to_string()];
        let b = matching_return_resource_plans(&plans, "shared-return", &b_context);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].relation["dummy_ret_state"], "proxy-B");

        let wrong_context = vec!["call-C".to_string()];
        assert!(matching_return_resource_plans(&plans, "shared-return", &wrong_context).is_empty());
    }

    #[test]
    fn p3_r2_spawn_callback_restores_caller_locals_and_joins_only_heap_effects() {
        let mut callback=plan("unit");
        callback.entry.activation["activation_kind"]=json!("higher_order_callback");
        callback.entry.activation["callback_semantics"]=json!("spawn_may_once");
        callback.destination=None;
        callback.return_shape="detached_callback_no_value_to_provider".into();

        let caller_resource=resource("caller-local");
        let callback_local=resource("callback-local");
        let heap_base=resource("heap-base");
        let heap_payload=resource("heap-payload");
        let mut caller=AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller",1),caller_resource.clone());
        let mut callee=AllocationIdentityMemory::default();
        callee.assign_fresh(var("callee",8),callback_local);
        callee.heap_values.insert(
            heap_base.clone(),
            HeapIdentityValue {
                points_to:BTreeMap::from([(vec![],BTreeSet::from([heap_payload.clone()]))]),
                ..HeapIdentityValue::default()
            },
        );
        let (out,record)=transfer_return_resource(&callback,&callee,&caller);
        assert_eq!(out.points_to(&var("caller",1)),BTreeSet::from([caller_resource]));
        assert!(out.points_to(&var("callee",8)).is_empty());
        assert_eq!(out.heap_values[&heap_base].points_to[&vec![]],BTreeSet::from([heap_payload]));
        assert_eq!(record["status"],"heap_effects_joined_into_caller_snapshot");
        assert_eq!(record["return_value_to_provider"],false);
    }

    #[test]
    fn p3_r2_alias_may_set_preserves_existing_ids_without_environment_join() {
        let a=resource("a");let b=resource("b");let resources=BTreeSet::from([a.clone(),b.clone()]);
        let mut callee=AllocationIdentityMemory::default();callee.assign_points_to(var("callee",0),resources.clone());callee.assign_fresh(var("callee",8),resource("unrelated"));
        let mut caller=AllocationIdentityMemory::default();caller.assign_fresh(var("caller",1),a);caller.assign_fresh(var("caller",3),resource("old_destination"));
        let (out,r)=transfer_return_resource(&plan("raw_pointer"),&callee,&caller);
        assert_eq!(out.points_to(&var("caller",3)),resources);assert!(out.points_to(&var("callee",8)).is_empty());assert_eq!(out.points_to(&var("caller",1)),caller.points_to(&var("caller",1)));assert_eq!(r["status"],"resource_binding_available");
    }
    #[test]
    fn p3_r2_distinct_activations_and_same_resource_are_not_rekeyed() {
        let caller=AllocationIdentityMemory::default();let mut a=AllocationIdentityMemory::default();let mut b=a.clone();
        a.assign_fresh(var("callee",0),resource("a"));b.assign_fresh(var("callee",0),resource("b"));
        let x=transfer_return_resource(&plan("raw_pointer"),&a,&caller).0;let y=transfer_return_resource(&plan("raw_pointer"),&b,&caller).0;
        assert_ne!(x.points_to(&var("caller",3)),y.points_to(&var("caller",3)));assert_eq!(x.points_to(&var("caller",3)),a.points_to(&var("callee",0)));
        let mut other=plan("raw_pointer");other.entry.activation["call_key"]=json!("K2");assert_eq!(transfer_return_resource(&other,&a,&caller).0.points_to(&var("caller",3)),x.points_to(&var("caller",3)));
    }
    #[test]
    fn p3_r2_scalar_resource_and_true_nonresource_overwrite_destination() {
        let mut callee=AllocationIdentityMemory::default();callee.assign_fresh(var("callee",0),resource("bits"));let mut caller=AllocationIdentityMemory::default();caller.assign_fresh(var("caller",3),resource("old"));
        let (out,r)=transfer_return_resource(&plan("scalar"),&callee,&caller);assert_eq!(out.points_to(&var("caller",3)),callee.points_to(&var("callee",0)));assert_eq!(r["status"],"resource_binding_available");
        let (out,r)=transfer_return_resource(&plan("scalar"),&AllocationIdentityMemory::default(),&caller);assert!(out.points_to(&var("caller",3)).is_empty());assert_eq!(r["status"],"no_tracked_resource");
    }
    #[test]
    fn p3_r2_callee_allocation_keeps_origin_and_does_not_capture_caller() {
        let created=resource("callee_site");let mut callee=AllocationIdentityMemory::default();callee.assign_fresh(var("callee",0),created.clone());let mut caller=AllocationIdentityMemory::default();caller.assign_fresh(var("caller",1),resource("caller_site"));
        let (out,_)=transfer_return_resource(&plan("owned_box"),&callee,&caller);assert_eq!(out.points_to(&var("caller",3)),BTreeSet::from([created]));assert_eq!(out.points_to(&var("caller",1)),caller.points_to(&var("caller",1)));
    }
    #[test]
    fn p3_r2_unsupported_and_missing_resource_fail_closed() {
        let (out,r)=transfer_return_resource(&plan("raw_pointer"),&AllocationIdentityMemory::default(),&AllocationIdentityMemory::default());assert!(out.points_to.is_empty());assert_eq!(r["status"],"unresolved_return");
        let mut p=plan("raw_pointer");p.destination=None;assert_eq!(transfer_return_resource(&p,&AllocationIdentityMemory::default(),&out).1["status"],"unsupported_destination_shape");
        assert_eq!(transfer_return_resource(&plan("opaque"),&AllocationIdentityMemory::default(),&out).1["status"],"unsupported_value_shape");
        assert_eq!(transfer_return_resource(&plan("aggregate"),&AllocationIdentityMemory::default(),&out).1["status"],"unresolved_return");
    }
    #[test]
    fn p3_r2_aggregate_projected_identity_is_rebased_to_destination() {
        let allocation=resource("aggregate_field");
        let source_place=PlaceId{base:var("callee",0),projection:vec![PlaceProjection::Field{index:1}]};
        let destination_place=PlaceId{base:var("caller",3),projection:vec![PlaceProjection::Field{index:1}]};
        let mut callee=AllocationIdentityMemory::default();
        callee.assign_place_points_to(source_place,BTreeSet::from([allocation.clone()]));
        let caller=AllocationIdentityMemory::default();
        let (out,r)=transfer_return_resource(&plan("aggregate"),&callee,&caller);
        assert_eq!(r["status"],"resource_binding_available");
        assert!(out.points_to(&var("caller",3)).is_empty());
        assert_eq!(out.points_to_place(&destination_place),BTreeSet::from([allocation]));
    }
    #[test]
    fn p3_r2_projection_is_exact_and_never_collapsed_to_base() {
        let mut callee=AllocationIdentityMemory::default();callee.assign_fresh(var("callee",0),resource("a"));let mut caller=AllocationIdentityMemory::default();caller.assign_fresh(var("caller",3),resource("base"));let mut p=plan("raw_pointer");p.destination.as_mut().unwrap().projection=vec![PlaceProjection::Field{index:1}];
        let (out,r)=transfer_return_resource(&p,&callee,&caller);assert_eq!(out.points_to(&var("caller",3)),caller.points_to(&var("caller",3)));assert_eq!(out.points_to_place(p.destination.as_ref().unwrap()),callee.points_to(&var("callee",0)));assert_eq!(r["status"],"resource_binding_available");
    }
    #[test]
    fn p3_r2_reference_cannot_import_general_caller_side_effect() {
        let place=PlaceId{base:var("caller",1),projection:vec![]};let mut caller=AllocationIdentityMemory::default();caller.assign_fresh(place.base.clone(),resource("before"));let mut callee=caller.clone();callee.assign_fresh(place.base.clone(),resource("changed"));callee.assign_stack_refs(var("callee",0),BTreeSet::from([place]));
        let (out,r)=transfer_return_resource(&plan("shared_reference"),&callee,&caller);assert_eq!(r["status"],"unresolved_return");assert_eq!(out.points_to(&var("caller",1)),caller.points_to(&var("caller",1)));assert!(out.points_to(&var("caller",3)).is_empty());
    }

    #[test]
    fn ht2_r2_return_preserves_heap_effect_without_importing_callee_locals() {
        let backing = resource("heap-M");
        let payload = resource("heap-R");
        let caller = AllocationIdentityMemory::default();
        let mut callee = AllocationIdentityMemory::default();
        callee.assign_fresh(var("callee",8),resource("callee-local"));
        callee.heap_values.insert(
            backing.clone(),
            HeapIdentityValue {
                points_to: BTreeMap::from([(
                    vec![PlaceProjection::Field { index: 0 }],
                    BTreeSet::from([payload.clone()]),
                )]),
                ..HeapIdentityValue::default()
            },
        );

        let (out, record) = transfer_return_resource(&plan("unit"), &callee, &caller);

        assert_eq!(record["status"], "no_tracked_resource");
        assert!(out.points_to(&var("callee",8)).is_empty());
        assert_eq!(
            out.heap_values[&backing].points_to
                [&vec![PlaceProjection::Field { index: 0 }]],
            BTreeSet::from([payload]),
        );
    }
}

#[cfg(test)]
mod p3_r1p_tests {
    use super::*;
    use crate::structs::{
        AllocationSiteId, RustPlaceRelationEvidence, SourceInfoData,
    };

    fn var(function: &str, local: u32) -> ProgramVarId {
        ProgramVarId::Rust { function: function.into(), local }
    }

    fn alloc(label: &str) -> AbstractAllocId {
        AbstractAllocId {
            site: AllocationSiteId::Synthetic {
                scope: "p3-r1p-test".into(),
                label: label.into(),
            },
            context: Vec::new(),
        }
    }

    fn fields(base: ProgramVarId, indices: &[u32]) -> PlaceId {
        PlaceId {
            base,
            projection: indices
                .iter()
                .map(|index| PlaceProjection::Field { index: *index })
                .collect(),
        }
    }

    fn box_pointer_evidence(source_local: u32, destination_local: u32) -> RustPlaceRelationEvidence {
        RustPlaceRelationEvidence {
            kind: RustPlaceRelationEvidenceKind::OwnedBoxDataPointerCopyForDeref,
            source_local,
            destination_local,
            source_projection: vec![
                PlaceProjection::Deref,
                PlaceProjection::Field { index: 0 },
                PlaceProjection::Field { index: 0 },
                PlaceProjection::Field { index: 0 },
            ],
            owner_def_path: "audit-only-owned-box".into(),
            source_type_diagnostic: "*mut Box<T>".into(),
            result_type_diagnostic: "*const T".into(),
            transparent_wrapper_depth: 2,
        }
    }

    fn certified_stmt(source_local: u32, destination_local: u32) -> MirStatement {
        MirStatement {
            resource_place_relation_v1: Some(box_pointer_evidence(source_local, destination_local)),
            source_info: SourceInfoData { span: "test".into(), scope: "test".into() },
            kind: "Assign".into(),
            details: "producer-certified; rendered text is audit-only".into(),
            place: Some(format!("Local(_{destination_local})")),
            is_mutable: Some(true),
            rvalue: Some("intentionally_not_parsed_by_P3_R1P".into()),
        }
    }

    #[test]
    fn direct_pointer_deref_preserves_exact_may_set() {
        let p = var("f", 1);
        let expected = BTreeSet::from([alloc("A"), alloc("B")]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(p.clone(), expected.clone());

        let query = PlaceId { base: p, projection: vec![PlaceProjection::Deref] };
        assert_eq!(
            resolve_resource_place(&memory, &query),
            ResourcePlaceResolution::Resolved(expected)
        );
    }

    #[test]
    fn exact_pointer_field_deref_is_projection_specific() {
        let owner = var("f", 1);
        let a = alloc("A");
        let b = alloc("B");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_place_points_to(fields(owner.clone(), &[0]), BTreeSet::from([a.clone()]));
        memory.assign_place_points_to(fields(owner.clone(), &[1]), BTreeSet::from([b.clone()]));

        let qa = PlaceId {
            base: owner.clone(),
            projection: vec![PlaceProjection::Field { index: 0 }, PlaceProjection::Deref],
        };
        let qb = PlaceId {
            base: owner,
            projection: vec![PlaceProjection::Field { index: 1 }, PlaceProjection::Deref],
        };
        assert_eq!(resolve_resource_place(&memory, &qa), ResourcePlaceResolution::Resolved(BTreeSet::from([a])));
        assert_eq!(resolve_resource_place(&memory, &qb), ResourcePlaceResolution::Resolved(BTreeSet::from([b])));
    }

    #[test]
    fn stack_reference_deref_uses_only_exact_referent() {
        let p = var("f", 1);
        let owner = var("f", 9);
        let target = fields(owner, &[2]);
        let expected = BTreeSet::from([alloc("R")]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_stack_refs(p.clone(), BTreeSet::from([target.clone()]));
        memory.assign_place_points_to(target, expected.clone());

        let query = PlaceId { base: p, projection: vec![PlaceProjection::Deref] };
        assert_eq!(resolve_resource_place(&memory, &query), ResourcePlaceResolution::Resolved(expected));
    }

    #[test]
    fn exact_stack_referent_can_continue_below_deref_despite_carried_resource_relation() {
        let receiver = var("callee", 1);
        let caller_owner = var("caller", 4);
        let field_resource = alloc("field-R");
        let carried_resource = alloc("container-R");
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(receiver.clone(), BTreeSet::from([carried_resource]));
        memory.assign_stack_refs(
            receiver.clone(),
            BTreeSet::from([PlaceId {
                base: caller_owner.clone(),
                projection: Vec::new(),
            }]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: caller_owner,
                projection: vec![PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([field_resource.clone()]),
        );

        let query = PlaceId {
            base: receiver,
            projection: vec![
                PlaceProjection::Deref,
                PlaceProjection::Field { index: 0 },
            ],
        };
        assert_eq!(
            resolve_resource_place(&memory, &query),
            ResourcePlaceResolution::Resolved(BTreeSet::from([field_resource])),
        );
    }

    #[test]
    fn unknown_and_unsupported_places_fail_closed() {
        let p = var("f", 1);
        let memory = AllocationIdentityMemory::default();
        let unknown = PlaceId { base: p.clone(), projection: vec![PlaceProjection::Deref] };
        assert_eq!(resolve_resource_place(&memory, &unknown), ResourcePlaceResolution::UnresolvedIdentity);

        let unsupported = PlaceId {
            base: p,
            projection: vec![PlaceProjection::Index { local: var("f", 2) }],
        };
        assert_eq!(resolve_resource_place(&memory, &unsupported), ResourcePlaceResolution::UnsupportedPlace);
    }

    #[test]
    fn heap_field_after_deref_is_not_invented() {
        let p = var("f", 1);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(p.clone(), BTreeSet::from([alloc("heap")])) ;
        let query = PlaceId {
            base: p,
            projection: vec![PlaceProjection::Deref, PlaceProjection::Field { index: 0 }],
        };
        assert_eq!(resolve_resource_place(&memory, &query), ResourcePlaceResolution::UnresolvedIdentity);
    }

    #[test]
    fn certified_box_pointer_carrier_populates_raw_pointer_then_deref() {
        let source = var("glue", 1);
        let destination = var("glue", 6);
        let expected = BTreeSet::from([alloc("A"), alloc("B")]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(source, expected.clone());

        assert!(apply_certified_place_relation("glue", &certified_stmt(1, 6), &mut memory));
        assert_eq!(memory.points_to(&destination), expected.clone());
        assert!(memory.stack_refs(&destination).is_empty());

        let dropped = PlaceId { base: destination, projection: vec![PlaceProjection::Deref] };
        assert_eq!(memory.resources_for_place(&dropped), expected.clone());
        assert_eq!(resolve_resource_place(&memory, &dropped), ResourcePlaceResolution::Resolved(expected));
    }

    #[test]
    fn malformed_certification_fails_closed_without_overwriting_destination() {
        let destination = var("glue", 6);
        let existing = BTreeSet::from([alloc("existing")]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(destination.clone(), existing.clone());
        let mut statement = certified_stmt(1, 6);
        statement
            .resource_place_relation_v1
            .as_mut()
            .unwrap()
            .source_projection[1] = PlaceProjection::Field { index: 1 };

        assert!(!apply_certified_place_relation("glue", &statement, &mut memory));
        assert_eq!(memory.points_to(&destination), existing);
        assert!(memory.resources_for_place(&PlaceId {
            base: destination,
            projection: vec![PlaceProjection::Deref],
        }).is_empty());
    }

    #[test]
    fn saved_box_blocker_source_shape_materializes_exact_pointee_resource() {
        // Mirrors the decisive blocker shape: the DropGlue receiver local has
        // both a canonical resource relation and an exact stack referent to the
        // caller Box, while the CopyForDeref destination starts empty.
        let receiver = var("drop_glue_box", 1);
        let caller_box = var("caller", 4);
        let raw = var("drop_glue_box", 6);
        let a = alloc("box_new_A");
        let b = alloc("box_new_B");
        let expected = BTreeSet::from([a, b]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(receiver.clone(), expected.clone());
        memory.assign_stack_refs(
            receiver,
            BTreeSet::from([PlaceId {
                base: caller_box.clone(),
                projection: Vec::new(),
            }]),
        );
        memory.assign_points_to(caller_box, expected.clone());

        assert!(apply_certified_place_relation(
            "drop_glue_box",
            &certified_stmt(1, 6),
            &mut memory,
        ));

        assert_eq!(memory.points_to(&raw), expected.clone());
        let pointee = PlaceId {
            base: raw,
            projection: vec![PlaceProjection::Deref],
        };
        assert_eq!(memory.resources_for_place(&pointee), expected.clone());
        assert_eq!(
            resolve_resource_place(&memory, &pointee),
            ResourcePlaceResolution::Resolved(expected),
        );
    }

    #[test]
    fn certified_carrier_can_read_exact_box_referent_without_copying_stack_identity() {
        let receiver = var("glue", 1);
        let caller_box = var("caller", 4);
        let raw = var("glue", 6);
        let expected = BTreeSet::from([alloc("R")]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(caller_box.clone(), expected.clone());
        memory.assign_stack_refs(receiver, BTreeSet::from([PlaceId { base: caller_box, projection: Vec::new() }]));

        assert!(apply_certified_place_relation("glue", &certified_stmt(1, 6), &mut memory));
        assert_eq!(memory.points_to(&raw), expected);
        assert!(memory.stack_refs(&raw).is_empty(), "raw heap pointer must not become a stack reference");
    }

    #[test]
    fn certified_carrier_does_not_upgrade_access_provenance_or_fabricate_empty_source() {
        let source = var("glue", 1);
        let destination = var("glue", 6);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_access_bases(source, BTreeSet::from([alloc("access-only")]));

        assert!(apply_certified_place_relation("glue", &certified_stmt(1, 6), &mut memory));
        assert!(memory.points_to(&destination).is_empty());
        let query = PlaceId { base: destination, projection: vec![PlaceProjection::Deref] };
        assert_eq!(resolve_resource_place(&memory, &query), ResourcePlaceResolution::UnresolvedIdentity);
    }

    #[test]
    fn place_resource_component_is_a_may_lattice_and_forget_is_strong() {
        let base = var("f", 1);
        let place = PlaceId {
            base: base.clone(),
            projection: vec![PlaceProjection::Deref],
        };
        let a = alloc("A");
        let b = alloc("B");

        let mut left = AllocationIdentityMemory::default();
        left.assign_place_resources(place.clone(), BTreeSet::from([a.clone()]));
        let mut right = AllocationIdentityMemory::default();
        right.assign_place_resources(place.clone(), BTreeSet::from([b.clone()]));

        let joined = left.join(&right);
        assert_eq!(
            joined.resources_for_place(&place),
            BTreeSet::from([a.clone(), b.clone()]),
        );
        assert!(left.leq(&joined));
        assert!(right.leq(&joined));
        assert!(!joined.leq(&left));

        let mut forgotten = joined;
        forgotten.forget_var(&base);
        assert!(forgotten.resources_for_place(&place).is_empty());
    }

    #[test]
    fn diagnostic_strings_are_not_semantic_inputs() {
        let source = var("glue", 1);
        let destination = var("glue", 6);
        let expected = BTreeSet::from([alloc("R")]);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(source, expected.clone());

        let mut statement = certified_stmt(1, 6);
        let evidence = statement.resource_place_relation_v1.as_mut().unwrap();
        evidence.owner_def_path = "not-a-box-name".into();
        evidence.source_type_diagnostic = "deliberately-mutated-audit-text".into();
        evidence.result_type_diagnostic = "also-not-semantic".into();
        statement.place = Some("untrusted rendered destination text".into());
        statement.rvalue = Some("untrusted rendered rvalue text".into());

        assert!(apply_certified_place_relation("glue", &statement, &mut memory));
        assert_eq!(memory.points_to(&destination), expected);
    }

    #[test]
    fn absent_place_resource_relation_does_not_change_legacy_json_shape() {
        let dump = IdentityMemoryDump::from_memory(&AllocationIdentityMemory::default());
        let json = serde_json::to_value(dump).expect("serialize identity memory dump");
        assert!(json.get("place_resources").is_none());

        let statement = MirStatement {
            resource_place_relation_v1: None,
            source_info: SourceInfoData { span: "test".into(), scope: "test".into() },
            kind: "Nop".into(),
            details: "Nop".into(),
            place: None,
            is_mutable: None,
            rvalue: None,
        };
        let statement_json = serde_json::to_value(statement).expect("serialize MIR statement");
        assert!(statement_json.get("resource_place_relation_v1").is_none());
    }

    #[test]
    fn aliases_preserve_one_canonical_id_and_distinct_resources_do_not_merge() {
        let r = alloc("shared");
        let a = var("f", 1);
        let b = var("f", 2);
        let c = var("f", 3);
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(a.clone(), BTreeSet::from([r.clone()]));
        memory.assign_points_to(b.clone(), BTreeSet::from([r.clone()]));
        memory.assign_points_to(c.clone(), BTreeSet::from([alloc("other")]));

        for alias in [a, b] {
            let q = PlaceId { base: alias, projection: vec![PlaceProjection::Deref] };
            assert_eq!(resolve_resource_place(&memory, &q), ResourcePlaceResolution::Resolved(BTreeSet::from([r.clone()])));
        }
        let q = PlaceId { base: c, projection: vec![PlaceProjection::Deref] };
        assert_eq!(resolve_resource_place(&memory, &q), ResourcePlaceResolution::Resolved(BTreeSet::from([alloc("other")])));
    }
}

#[cfg(test)]
mod p3_r1d_tests {
    use super::*;
    use crate::structs::AllocationSiteId;

    fn var(function: &str, local: u32) -> ProgramVarId {
        ProgramVarId::Rust { function: function.into(), local }
    }

    fn resource(label: &str) -> AbstractAllocId {
        AbstractAllocId {
            site: AllocationSiteId::RustCall {
                node_id: label.into(),
                callee: "std::alloc::alloc".into(),
            },
            context: Vec::new(),
        }
    }

    fn place(base: ProgramVarId, projection: Vec<PlaceProjection>) -> PlaceId {
        PlaceId { base, projection }
    }

    fn field(base: ProgramVarId, indices: &[u32]) -> PlaceId {
        PlaceId {
            base,
            projection: indices
                .iter()
                .map(|index| PlaceProjection::Field { index: *index })
                .collect(),
        }
    }

    fn plan(key: &str, local: u32, projection: serde_json::Value) -> DropReceiverPlan {
        DropReceiverPlan {
            activation: serde_json::json!({
                "call_key": key,
                "activation_kind": "mir_drop",
                "caller_instance_id": "native-caller",
                "callee_instance_id": "native-glue",
                "parent_context": ["outer"],
                "callee_context": ["outer", key],
                "caller_call_state": "caller-state",
                "callee_entry_state": "entry",
            }),
            caller_scope: "caller".into(),
            place: serde_json::json!({
                "local": local,
                "projection": projection,
                "source": "rustc_mir_Drop_Place",
            }),
            receiver: var(key, 1),
        }
    }

    #[test]
    fn saved_box_terminal_deref_consumes_place1_resource_relation() {
        let mut caller = AllocationIdentityMemory::default();
        let r = resource("R");
        let dropped = place(var("caller", 6), vec![PlaceProjection::Deref]);
        caller.assign_place_resources(dropped.clone(), BTreeSet::from([r.clone()]));

        let p = plan("KD", 6, serde_json::json!([{"kind":"deref"}]));
        let (entry, record) = transfer_drop_receiver(&p, &caller);

        assert_eq!(record["status"], "resource_binding_available");
        assert_eq!(entry.points_to(&p.receiver), BTreeSet::from([r.clone()]));
        assert_eq!(entry.stack_refs(&p.receiver), BTreeSet::from([dropped]));
        assert_eq!(
            entry.resources_for_place(&place(
                p.receiver.clone(),
                vec![PlaceProjection::Deref]
            )),
            BTreeSet::from([r]),
        );
    }

    #[test]
    fn direct_drop_receiver_preserves_same_canonical_resource() {
        let mut caller = AllocationIdentityMemory::default();
        let r = resource("A");
        caller.assign_fresh(var("caller", 1), r.clone());
        let p = plan("KD", 1, serde_json::json!([]));
        let (entry, record) = transfer_drop_receiver(&p, &caller);
        assert_eq!(record["status"], "resource_binding_available");
        assert_eq!(entry.points_to(&p.receiver), BTreeSet::from([r]));
        assert_eq!(entry.stack_refs(&p.receiver), BTreeSet::from([field(var("caller", 1), &[])]));
    }

    #[test]
    fn may_drop_set_is_preserved_without_collapse() {
        let mut caller = AllocationIdentityMemory::default();
        let expected = BTreeSet::from([resource("A"), resource("B")]);
        caller.assign_points_to(var("caller", 1), expected.clone());
        let p = plan("KD", 1, serde_json::json!([]));
        let (entry, _) = transfer_drop_receiver(&p, &caller);
        assert_eq!(entry.points_to(&p.receiver), expected);
    }

    #[test]
    fn terminal_raw_pointer_deref_preserves_place1_may_set() {
        let mut caller = AllocationIdentityMemory::default();
        let expected = BTreeSet::from([resource("A"), resource("B")]);
        caller.assign_points_to(var("caller", 6), expected.clone());
        let p = plan("KD", 6, serde_json::json!([{"kind":"deref"}]));
        let (entry, record) = transfer_drop_receiver(&p, &caller);
        assert_eq!(record["status"], "resource_binding_available");
        assert_eq!(entry.points_to(&p.receiver), expected);
    }

    #[test]
    fn distinct_drop_activations_do_not_cross_bind() {
        let mut a = AllocationIdentityMemory::default();
        let mut b = AllocationIdentityMemory::default();
        a.assign_fresh(var("caller", 1), resource("A"));
        b.assign_fresh(var("caller", 1), resource("B"));
        let pa = plan("KDA", 1, serde_json::json!([]));
        let pb = plan("KDB", 1, serde_json::json!([]));
        assert_eq!(transfer_drop_receiver(&pa, &a).0.points_to(&pa.receiver), BTreeSet::from([resource("A")]));
        assert_eq!(transfer_drop_receiver(&pb, &b).0.points_to(&pb.receiver), BTreeSet::from([resource("B")]));
    }

    #[test]
    fn same_resource_is_not_rekeyed_by_drop_context() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller", 1), resource("R"));
        for key in ["KDA", "KDB"] {
            let p = plan(key, 1, serde_json::json!([]));
            assert_eq!(transfer_drop_receiver(&p, &caller).0.points_to(&p.receiver), BTreeSet::from([resource("R")]));
        }
    }

    #[test]
    fn exact_fields_remain_projection_specific() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_place_resources(field(var("caller", 1), &[0]), BTreeSet::from([resource("A")]));
        caller.assign_place_resources(field(var("caller", 1), &[1]), BTreeSet::from([resource("B")]));

        for (index, label) in [(0, "A"), (1, "B")] {
            let p = plan("KD", 1, serde_json::json!([{"kind":"field","index":index}]));
            let (entry, record) = transfer_drop_receiver(&p, &caller);
            assert_eq!(record["status"], "resource_binding_available");
            assert_eq!(entry.points_to(&p.receiver), BTreeSet::from([resource(label)]));
        }
    }

    #[test]
    fn field_owner_imports_only_its_existing_structural_subtree() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_place_points_to(field(var("caller", 1), &[0, 0]), BTreeSet::from([resource("A")]));
        caller.assign_place_points_to(field(var("caller", 1), &[1, 0]), BTreeSet::from([resource("B")]));

        let a = plan("KA", 1, serde_json::json!([{"kind":"field","index":0}]));
        let b = plan("KB", 1, serde_json::json!([{"kind":"field","index":1}]));
        let (a_entry, a_record) = transfer_drop_receiver(&a, &caller);
        let (b_entry, b_record) = transfer_drop_receiver(&b, &caller);

        assert_eq!(a_record["status"], "resource_binding_available");
        assert_eq!(b_record["status"], "resource_binding_available");
        assert_eq!(a_entry.points_to(&a.receiver), BTreeSet::from([resource("A")]));
        assert_eq!(b_entry.points_to(&b.receiver), BTreeSet::from([resource("B")]));
        assert!(!a_entry.points_to(&a.receiver).contains(&resource("B")));
        assert!(!b_entry.points_to(&b.receiver).contains(&resource("A")));
    }

    #[test]
    fn nested_deref_field_uses_only_exact_stack_referent() {
        let mut caller = AllocationIdentityMemory::default();
        let referent = field(var("app", 8), &[2]);
        caller.assign_stack_refs(var("caller", 1), BTreeSet::from([referent]));
        caller.assign_place_resources(field(var("app", 8), &[2, 1]), BTreeSet::from([resource("R")]));
        let p = plan("KD", 1, serde_json::json!([
            {"kind":"deref"},
            {"kind":"field","index":1}
        ]));
        let (entry, record) = transfer_drop_receiver(&p, &caller);
        assert_eq!(record["status"], "resource_binding_available");
        assert_eq!(entry.points_to(&p.receiver), BTreeSet::from([resource("R")]));
    }

    #[test]
    fn heap_field_after_unlocated_deref_remains_unresolved() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_points_to(var("caller", 1), BTreeSet::from([resource("R")]));
        let p = plan("KD", 1, serde_json::json!([
            {"kind":"deref"},
            {"kind":"field","index":0}
        ]));
        let (_, record) = transfer_drop_receiver(&p, &caller);
        assert_eq!(record["status"], "unresolved_identity");
    }

    #[test]
    fn direct_nonresource_is_proved_empty_without_fabrication() {
        let p = plan("KD", 1, serde_json::json!([]));
        let (entry, record) = transfer_drop_receiver(&p, &AllocationIdentityMemory::default());
        assert_eq!(record["status"], "no_tracked_resource");
        assert!(entry.points_to(&p.receiver).is_empty());
        assert_eq!(entry.stack_refs(&p.receiver), BTreeSet::from([field(var("caller", 1), &[])]));
    }

    #[test]
    fn unproved_deref_is_unresolved_not_successful_empty() {
        let p = plan("KD", 1, serde_json::json!([{"kind":"deref"}]));
        let (entry, record) = transfer_drop_receiver(&p, &AllocationIdentityMemory::default());
        assert_eq!(record["status"], "unresolved_identity");
        assert!(entry.points_to(&p.receiver).is_empty());
        assert!(entry.stack_refs(&p.receiver).is_empty());
    }

    #[test]
    fn unsupported_projection_is_fail_closed() {
        let p = plan("KD", 1, serde_json::json!([{"kind":"unsupported","diagnostic":"Index"}]));
        let (_, record) = transfer_drop_receiver(&p, &AllocationIdentityMemory::default());
        assert_eq!(record["status"], "unsupported_place");
    }

    #[test]
    fn place_resource_descendants_are_rebased_below_receiver_deref() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_place_resources(field(var("caller", 1), &[0]), BTreeSet::from([resource("ROOT")]));
        caller.assign_place_resources(field(var("caller", 1), &[0, 2]), BTreeSet::from([resource("CHILD")]));
        let p = plan("KD", 1, serde_json::json!([{"kind":"field","index":0}]));
        let (entry, _) = transfer_drop_receiver(&p, &caller);
        assert_eq!(
            entry.resources_for_place(&place(
                p.receiver.clone(),
                vec![PlaceProjection::Deref, PlaceProjection::Field { index: 2 }]
            )),
            BTreeSet::from([resource("CHILD")]),
        );
    }

    #[test]
    fn drop_receiver_composes_with_ordinary_r1_without_new_resource() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_place_resources(field(var("caller", 1), &[0]), BTreeSet::from([resource("R")]));
        let p = plan("KD", 1, serde_json::json!([{"kind":"field","index":0}]));
        let (mut entry, _) = transfer_drop_receiver(&p, &caller);
        entry.assign_stack_refs(
            var("KD", 2),
            BTreeSet::from([PlaceId { base: p.receiver.clone(), projection: Vec::new() }]),
        );
        let r1 = ActualFormalPlan {
            activation: serde_json::json!({}),
            caller_scope: "KD".into(),
            callee_scope: "dtor".into(),
            actuals: vec![serde_json::json!({
                "actual_local": 2,
                "operand": "copy",
                "direct_local": true,
                "value_shape": "mutable_reference"
            })],
            formals: vec![serde_json::json!({"value_shape":"mutable_reference"})],
            abi: "Rust".into(),
        };
        let (dtor, records) = transfer_actual_formal(&r1, &entry);
        assert_eq!(records[0]["resources"], serde_json::json!([resource("R")]));
        assert_eq!(dtor.event_allocations(&var("dtor", 1)), BTreeSet::from([resource("R")]));
    }

    #[test]
    fn moved_away_resource_is_not_recreated_at_drop_boundary() {
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller", 1), resource("R"));
        caller.forget_var(&var("caller", 1));
        let p = plan("KD", 1, serde_json::json!([]));
        let (entry, record) = transfer_drop_receiver(&p, &caller);
        assert_eq!(record["status"], "no_tracked_resource");
        assert!(entry.points_to(&p.receiver).is_empty());
    }

    #[test]
    fn reused_drop_glue_return_is_selected_by_exact_activation_context() {
        let mut a = plan("KA", 1, serde_json::json!([]));
        let mut b = plan("KB", 1, serde_json::json!([]));

        // IdentityAnalysis uses the context-qualified P2 caller execution-state
        // as its k=1 callee context at DropGlue entry and again at return.
        // The call key / reused callee body is therefore not sufficient to
        // select a return continuation.
        a.activation["caller_call_state"] = serde_json::json!("caller-A");
        b.activation["caller_call_state"] = serde_json::json!("caller-B");

        assert_eq!(drop_callee_context(&a), Some(vec!["caller-A".to_string()]));
        assert_eq!(drop_callee_context(&b), Some(vec!["caller-B".to_string()]));
        assert_ne!(drop_callee_context(&a), drop_callee_context(&b));

        let mut same_activation_context = b.clone();
        same_activation_context.activation["call_key"] = serde_json::json!("K-OTHER");
        assert_eq!(
            drop_callee_context(&b),
            drop_callee_context(&same_activation_context),
        );
    }

    #[test]
    fn unwind_pop_does_not_consume_parallel_normal_callee_successor() {
        let nexts = BTreeSet::from([
            "caller-cleanup".to_string(),
            "callee-normal".to_string(),
        ]);
        let matched_unwind = BTreeSet::from(["caller-cleanup".to_string()]);

        assert_eq!(
            drop_same_context_successors(&nexts, &matched_unwind),
            BTreeSet::from(["callee-normal".to_string()]),
        );
    }

    #[test]
    fn unwind_only_exit_does_not_reenter_caller_with_callee_context() {
        let nexts = BTreeSet::from(["caller-cleanup".to_string()]);
        let matched_unwind = BTreeSet::from(["caller-cleanup".to_string()]);

        assert!(drop_same_context_successors(&nexts, &matched_unwind).is_empty());
    }

    #[test]
    fn saved_box_chain_outer_drop_place1_copy_for_deref_inner_drop_keeps_one_id() {
        use crate::structs::{RustPlaceRelationEvidence, RustPlaceRelationEvidenceKind, SourceInfoData};

        let r = resource("BOX-R");
        let mut caller = AllocationIdentityMemory::default();
        caller.assign_fresh(var("caller", 4), r.clone());

        // Outer MIR Drop binds the Box storage to the compiler-generated Box
        // DropGlue receiver.
        let outer = DropReceiverPlan {
            activation: serde_json::json!({
                "call_key":"K-OUTER","activation_kind":"mir_drop",
                "caller_instance_id":"caller-instance","callee_instance_id":"box-glue",
                "parent_context":[],"callee_context":["K-OUTER"],
                "caller_call_state":"caller-bb","callee_entry_state":"box-entry"
            }),
            caller_scope: "caller".into(),
            place: serde_json::json!({"local":4,"projection":[],"source":"rustc_mir_Drop_Place"}),
            receiver: var("box-glue", 1),
        };
        let (mut box_entry, outer_record) = transfer_drop_receiver(&outer, &caller);
        assert_eq!(outer_record["status"], "resource_binding_available");
        assert_eq!(box_entry.points_to(&var("box-glue", 1)), BTreeSet::from([r.clone()]));

        // PLACE1 producer-certified CopyForDeref carries the same allocation
        // into Box glue's raw data pointer `_6` and materializes `resources(*_6)`.
        let stmt = MirStatement {
            source_info: SourceInfoData { span: "test".into(), scope: "test".into() },
            kind: "Assign".into(),
            details: "audit-only".into(),
            place: Some("audit-only".into()),
            is_mutable: Some(true),
            rvalue: Some("audit-only".into()),
            resource_place_relation_v1: Some(RustPlaceRelationEvidence {
                kind: RustPlaceRelationEvidenceKind::OwnedBoxDataPointerCopyForDeref,
                source_local: 1,
                destination_local: 6,
                source_projection: vec![
                    PlaceProjection::Deref,
                    PlaceProjection::Field { index: 0 },
                    PlaceProjection::Field { index: 0 },
                    PlaceProjection::Field { index: 0 },
                ],
                owner_def_path: "diagnostic-only".into(),
                source_type_diagnostic: "diagnostic-only".into(),
                result_type_diagnostic: "diagnostic-only".into(),
                transparent_wrapper_depth: 2,
            }),
        };
        assert!(apply_certified_place_relation("box-glue", &stmt, &mut box_entry));
        assert_eq!(
            box_entry.resources_for_place(&place(
                var("box-glue", 6),
                vec![PlaceProjection::Deref]
            )),
            BTreeSet::from([r.clone()]),
        );

        // The child `Drop(*_6)` must consume that authoritative PLACE1 fact
        // and bind the child DropGlue receiver to the exact same canonical R.
        let child = DropReceiverPlan {
            activation: serde_json::json!({
                "call_key":"K-CHILD","activation_kind":"mir_drop",
                "caller_instance_id":"box-glue","callee_instance_id":"u64-glue",
                "parent_context":["K-OUTER"],"callee_context":["K-OUTER","K-CHILD"],
                "caller_call_state":"box-bb0","callee_entry_state":"u64-entry"
            }),
            caller_scope: "box-glue".into(),
            place: serde_json::json!({
                "local":6,"projection":[{"kind":"deref"}],"source":"rustc_mir_Drop_Place"
            }),
            receiver: var("u64-glue", 1),
        };
        let (child_entry, child_record) = transfer_drop_receiver(&child, &box_entry);
        assert_eq!(child_record["status"], "resource_binding_available");
        assert_eq!(child_record["resources"], serde_json::json!([r.clone()]));
        assert_eq!(child_entry.points_to(&var("u64-glue", 1)), BTreeSet::from([r]));
    }

}
