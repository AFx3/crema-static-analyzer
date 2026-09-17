use crate::identity::{AllocationIdentityMemory, AllocationIdentityState};
use crate::panic_unwind::{edge_flow_kind, EdgeFlowKind};
use crate::structs::{
    parse_mir_local_index, AbstractAllocId, GlobalICFGNode, GlobalICFGOrdered, IcfgEdge,
    MirStatement, MirTerminator, PlaceId, ProgramVarId,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Auxiliary may-domain for panic/unwind ownership lifecycle.
///
/// This domain is intentionally separate from `CellValue`: `CellValue`
/// describes the physical/abstract memory cell (ALLOC/FREED/TOP/...), while
/// this value records whether an owner may still be capable of destroying the
/// same resource and whether a panic happened after only partial destruction.
///
/// The order is pointwise implication over MAY facts (`false <= true`).
/// Therefore join is boolean OR for every component.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LifecycleValue {
    may_own: bool,
    may_partial_drop: bool,
    may_stale_owner: bool,
    may_committed: bool,
    may_complete: bool,
}

impl LifecycleValue {
    pub const fn bottom() -> Self {
        Self {
            may_own: false,
            may_partial_drop: false,
            may_stale_owner: false,
            may_committed: false,
            may_complete: false,
        }
    }

    pub const fn owned() -> Self {
        Self {
            may_own: true,
            ..Self::bottom()
        }
    }

    pub const fn may_own(self) -> bool {
        self.may_own
    }

    pub const fn may_partial_drop(self) -> bool {
        self.may_partial_drop
    }

    pub const fn may_stale_owner(self) -> bool {
        self.may_stale_owner
    }

    pub const fn may_committed(self) -> bool {
        self.may_committed
    }

    pub const fn may_complete(self) -> bool {
        self.may_complete
    }

    pub const fn leq(self, other: Self) -> bool {
        (!self.may_own || other.may_own)
            && (!self.may_partial_drop || other.may_partial_drop)
            && (!self.may_stale_owner || other.may_stale_owner)
            && (!self.may_committed || other.may_committed)
            && (!self.may_complete || other.may_complete)
    }

    pub const fn join(self, other: Self) -> Self {
        Self {
            may_own: self.may_own || other.may_own,
            may_partial_drop: self.may_partial_drop || other.may_partial_drop,
            may_stale_owner: self.may_stale_owner || other.may_stale_owner,
            may_committed: self.may_committed || other.may_committed,
            may_complete: self.may_complete || other.may_complete,
        }
    }

    /// Ownership was extracted from a container before the container metadata
    /// was made panic-safe.  The resource may still be considered owned by the
    /// stale container if unwinding observes this intermediate state.
    pub const fn extract_before_commit(mut self) -> Self {
        self.may_own = true;
        self.may_stale_owner = true;
        self.may_complete = false;
        self
    }

    /// A commit-before-drop step made the owner metadata consistent with the
    /// prefix/state that is safe to observe if a later destructor panics.
    ///
    /// This is a strong transfer for a definitely executed commit: stale
    /// metadata is cleared on this path. At CFG joins, `join` recovers the MAY
    /// stale fact if another incoming path did not execute the commit.
    pub const fn commit_before_drop(mut self) -> Self {
        self.may_committed = true;
        self.may_stale_owner = false;
        self.may_complete = false;
        self
    }

    /// A destructor panicked before normal drop completion.
    pub const fn drop_unwind(mut self) -> Self {
        self.may_partial_drop = true;
        self.may_complete = false;
        self
    }

    /// Normal drop completion consumes this owner's destruction capability.
    pub const fn drop_return(mut self) -> Self {
        self.may_own = false;
        self.may_partial_drop = false;
        self.may_stale_owner = false;
        self.may_complete = true;
        self
    }

    /// Minimal A3.7 witness: after a partial drop, stale ownership may still
    /// authorize another destruction attempt of the same abstract resource.
    pub const fn may_repeat_drop(self) -> bool {
        self.may_own && self.may_partial_drop && self.may_stale_owner
    }
}

/// Precision/coverage component for the lifecycle producer.
///
/// `Complete` is the lattice bottom: every lifecycle-relevant operation seen on
/// the represented path was interpreted by this producer. `Unresolved` is the
/// top element and records that at least one relevant operation could not be
/// correlated to the abstract resource domain. Join is therefore max.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LifecycleCoverage {
    #[default]
    Complete,
    Unresolved,
}

impl LifecycleCoverage {
    pub const fn leq(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Complete, Self::Complete)
                | (Self::Complete, Self::Unresolved)
                | (Self::Unresolved, Self::Unresolved)
        )
    }

    pub const fn join(self, other: Self) -> Self {
        if matches!(self, Self::Unresolved) || matches!(other, Self::Unresolved) {
            Self::Unresolved
        } else {
            Self::Complete
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Sparse map from an ownership/resource key to lifecycle facts plus an
/// explicit producer-coverage bit. Facts and coverage are intentionally
/// orthogonal: absence of a MAY fact is a refutation only when coverage is
/// `Complete`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanicLifecycleMemory {
    state: BTreeMap<String, LifecycleValue>,
    coverage: LifecycleCoverage,
}

impl PanicLifecycleMemory {
    pub fn get(&self, key: &str) -> LifecycleValue {
        self.state
            .get(key)
            .copied()
            .unwrap_or_else(LifecycleValue::bottom)
    }

    pub const fn coverage(&self) -> LifecycleCoverage {
        self.coverage
    }

    pub fn mark_unresolved(&mut self) {
        self.coverage = LifecycleCoverage::Unresolved;
    }

    pub fn strong_update(&mut self, key: impl Into<String>, value: LifecycleValue) {
        let key = key.into();
        if value == LifecycleValue::bottom() {
            self.state.remove(&key);
        } else {
            self.state.insert(key, value);
        }
    }

    pub fn mark_owned(&mut self, key: impl Into<String>) {
        self.strong_update(key, LifecycleValue::owned());
    }

    pub fn extract_before_commit(&mut self, key: &str) {
        self.strong_update(key.to_string(), self.get(key).extract_before_commit());
    }

    pub fn commit_before_drop(&mut self, key: &str) {
        self.strong_update(key.to_string(), self.get(key).commit_before_drop());
    }

    pub fn drop_unwind(&mut self, key: &str) {
        self.strong_update(key.to_string(), self.get(key).drop_unwind());
    }

    pub fn drop_return(&mut self, key: &str) {
        self.strong_update(key.to_string(), self.get(key).drop_return());
    }

    /// Mark every currently stale owner as having observed a destructor unwind.
    ///
    /// The key models the owner/resource obligation, not the element being
    /// dropped. Therefore an unwind from `drop_in_place::<T>` advances the
    /// lifecycle of stale owners already in scope instead of requiring the
    /// dropped element to have the same allocation identity.
    pub fn mark_stale_owners_partial_drop(&mut self) -> usize {
        let keys: Vec<String> = self
            .state
            .iter()
            .filter_map(|(key, value)| value.may_stale_owner().then_some(key.clone()))
            .collect();
        let newly_partial = keys
            .iter()
            .filter(|key| !self.get(key).may_partial_drop())
            .count();
        for key in &keys {
            self.drop_unwind(key);
        }
        newly_partial
    }

    /// Deterministic read-only view used by schema-v2 export.
    pub fn iter(&self) -> impl Iterator<Item = (&str, LifecycleValue)> {
        self.state.iter().map(|(key, value)| (key.as_str(), *value))
    }

    pub fn leq(&self, other: &Self) -> bool {
        let keys: BTreeSet<_> = self
            .state
            .keys()
            .chain(other.state.keys())
            .map(String::as_str)
            .collect();
        self.coverage.leq(other.coverage)
            && keys.into_iter().all(|key| self.get(key).leq(other.get(key)))
    }

    pub fn join(&self, other: &Self) -> Self {
        let keys: BTreeSet<_> = self
            .state
            .keys()
            .chain(other.state.keys())
            .cloned()
            .collect();
        let mut out = Self::default();
        out.coverage = self.coverage.join(other.coverage);
        for key in keys {
            out.strong_update(key.clone(), self.get(&key).join(other.get(&key)));
        }
        out
    }
}

/// Program-point map for the lifecycle component.  This mirrors
/// `AbstractState`/`TaintState` but remains a separate lattice component.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanicLifecycleState {
    state_map: BTreeMap<String, PanicLifecycleMemory>,
}

impl PanicLifecycleState {
    pub fn get(&self, block: &str) -> PanicLifecycleMemory {
        self.state_map.get(block).cloned().unwrap_or_default()
    }

    pub fn insert(&mut self, block: impl Into<String>, memory: PanicLifecycleMemory) {
        self.state_map.insert(block.into(), memory);
    }

    pub fn contains_block(&self, block: &str) -> bool {
        self.state_map.contains_key(block)
    }

    pub fn join_into(&mut self, block: &str, candidate: &PanicLifecycleMemory) -> bool {
        let old = self.get(block);
        let joined = old.join(candidate);
        let changed = joined != old;
        if changed || !self.state_map.contains_key(block) {
            self.insert(block.to_string(), joined);
        }
        changed
    }
}



/// Run the lifecycle satellite-domain fixed point over the canonical ICFG.
///
/// Stage 2 deliberately keeps MIR/event interpretation outside this engine.
/// `edge_transfer` is the only hook that may change lifecycle facts while an
/// edge is traversed. This lets the same monotone propagation machinery be
/// tested with synthetic transfers now and wired to real MIR events later.
///
/// The seed is attached to `entry`. Successor states are joined pointwise, so
/// path merges preserve MAY evidence from every incoming path.
pub fn fixed_point_lifecycle_with_transfer<F>(
    icfg: &GlobalICFGOrdered,
    entry: &str,
    seed: &PanicLifecycleMemory,
    edge_transfer: F,
) -> Result<PanicLifecycleState, String>
where
    F: Fn(&IcfgEdge, &PanicLifecycleMemory) -> PanicLifecycleMemory,
{
    let node_ids: BTreeSet<String> = icfg
        .ordered_nodes
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    if !node_ids.contains(entry) {
        return Err(format!("lifecycle entry node '{entry}' is not present in the ICFG"));
    }

    let mut outgoing: BTreeMap<String, Vec<IcfgEdge>> = BTreeMap::new();
    for edge in &icfg.icfg_edges {
        if !node_ids.contains(&edge.source) || !node_ids.contains(&edge.destination) {
            return Err(format!(
                "lifecycle propagation edge is not closed over the node domain: '{}' -> '{}'",
                edge.source, edge.destination
            ));
        }
        outgoing
            .entry(edge.source.clone())
            .or_default()
            .push(edge.clone());
    }
    for edges in outgoing.values_mut() {
        edges.sort_by(|a, b| {
            (&a.destination, &a.label, &a.source_label, &a.destination_label)
                .cmp(&(&b.destination, &b.label, &b.source_label, &b.destination_label))
        });
    }

    let mut state = PanicLifecycleState::default();
    state.insert(entry.to_string(), seed.clone());
    let mut queued = BTreeSet::from([entry.to_string()]);
    let mut worklist = VecDeque::from([entry.to_string()]);

    while let Some(current) = worklist.pop_front() {
        queued.remove(&current);
        let current_memory = state.get(&current);
        let Some(edges) = outgoing.get(&current) else {
            continue;
        };

        for edge in edges {
            let candidate = edge_transfer(edge, &current_memory);
            let destination = edge.destination.as_str();
            let first = !state.contains_block(destination);
            let changed = state.join_into(destination, &candidate);
            if (first || changed) && queued.insert(destination.to_string()) {
                worklist.push_back(destination.to_string());
            }
        }
    }

    Ok(state)
}

/// Identity-transfer specialization used before real MIR lifecycle events are
/// wired in. It is useful as a regression oracle: facts already present at a
/// program point must survive ordinary canonical ICFG propagation unchanged.
pub fn fixed_point_lifecycle_passthrough(
    icfg: &GlobalICFGOrdered,
    entry: &str,
    seed: &PanicLifecycleMemory,
) -> Result<PanicLifecycleState, String> {
    fixed_point_lifecycle_with_transfer(icfg, entry, seed, |_edge, memory| memory.clone())
}


/// Stable opaque key shared with CQPL's `allocations[].id` representation.
pub fn lifecycle_key_for_allocation(allocation: &AbstractAllocId) -> String {
    serde_json::to_string(allocation).expect("AbstractAllocId must serialize")
}

fn mir_function_for_node(node_id: &str) -> Option<String> {
    let rest = node_id.strip_prefix("rust::")?;
    let (function, bb) = rest.rsplit_once("::bb")?;
    (!function.is_empty() && bb.chars().all(|ch| ch.is_ascii_digit()))
        .then_some(function.to_string())
}

fn identity_memory_at<'a>(
    identity: &'a AllocationIdentityState,
    node_id: &str,
) -> Option<&'a AllocationIdentityMemory> {
    identity
        .event_by_node
        .get(node_id)
        .or_else(|| identity.by_node.get(node_id))
}

fn projection_has_prefix(candidate: &PlaceId, prefix: &PlaceId) -> bool {
    candidate.base == prefix.base
        && candidate.projection.len() >= prefix.projection.len()
        && candidate.projection[..prefix.projection.len()] == prefix.projection[..]
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LifecycleSubjectResolution {
    allocations: Vec<AbstractAllocId>,
    /// The subject was correlated only at the owning-aggregate boundary, not
    /// at its exact projected field.  This remains sound for a MAY producer,
    /// but the lifecycle coverage must stay unresolved.
    owner_scoped_fallback: bool,
}

fn allocations_under_place(
    memory: &AllocationIdentityMemory,
    prefix: &PlaceId,
) -> BTreeSet<AbstractAllocId> {
    let mut out = memory.points_to_place(prefix);
    for (candidate, candidate_allocations) in &memory.place_points_to {
        if projection_has_prefix(candidate, prefix) {
            out.extend(candidate_allocations.iter().cloned());
        }
    }
    out
}

/// Resolve the owner allocation(s) denoted by a MIR lifecycle subject.
///
/// Resolution is deliberately two-tiered.  First we follow the exact MAY
/// stack-place relation, including rebasing projected places through already
/// proven references (for example a callee `self` back to the caller owner).
/// If that exact projected path has no represented allocation, we may widen
/// only to allocations below the *same proven owner aggregate*.  This handles
/// field-sensitive precision loss at interprocedural/aggregate boundaries
/// without falling back to every allocation represented at the program point.
///
/// The owner-scoped fallback never creates an `AbstractAllocId`.  It reuses
/// only identities already present in `AllocationIdentityMemory` and is marked
/// explicitly so the lifecycle producer can set coverage to `Unresolved`.
fn lifecycle_allocations_for_subject(
    node_id: &str,
    raw_subject: &str,
    identity: &AllocationIdentityState,
) -> LifecycleSubjectResolution {
    let Some(function) = mir_function_for_node(node_id) else {
        return LifecycleSubjectResolution::default();
    };
    let Some(var) = ProgramVarId::rust(function, raw_subject) else {
        return LifecycleSubjectResolution::default();
    };
    let Some(memory) = identity_memory_at(identity, node_id) else {
        return LifecycleSubjectResolution::default();
    };

    let mut exact = memory.event_allocations(&var);
    let mut pending: Vec<PlaceId> = memory.stack_refs(&var).into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut owner_roots = BTreeSet::new();

    while let Some(place) = pending.pop() {
        if !seen.insert(place.clone()) {
            continue;
        }

        // The base aggregate itself is the narrowest sound fallback boundary
        // for a projected reference if exact field identity is absent.
        owner_roots.insert(PlaceId {
            base: place.base.clone(),
            projection: Vec::new(),
        });

        exact.extend(memory.allocations_for_place(place.clone()));
        exact.extend(allocations_under_place(memory, &place));
        pending.extend(memory.stack_refs_place(&place));

        // Rebase `reference.field` through a proven relation for `reference`.
        // Keep the unprojected referent as an owner-scoped fallback root, while
        // the projected referent continues through exact resolution.
        for base_target in memory.stack_refs(&place.base) {
            owner_roots.insert(base_target.clone());
            let mut rebased = base_target;
            rebased.projection.extend(place.projection.iter().cloned());
            pending.push(rebased);
        }
    }

    if !exact.is_empty() {
        return LifecycleSubjectResolution {
            allocations: exact.into_iter().collect(),
            owner_scoped_fallback: false,
        };
    }

    let mut fallback = BTreeSet::new();
    for owner in owner_roots {
        fallback.extend(allocations_under_place(memory, &owner));
    }

    LifecycleSubjectResolution {
        owner_scoped_fallback: !fallback.is_empty(),
        allocations: fallback.into_iter().collect(),
    }
}

fn is_manually_drop_take(term: &MirTerminator) -> bool {
    matches!(
        term,
        MirTerminator::Call {
            callee_def_path: Some(path),
            ..
        } if path == "std::mem::ManuallyDrop::<T>::take"
    )
}

fn is_destructor_like_call(term: &MirTerminator) -> bool {
    matches!(term, MirTerminator::Drop { .. })
        || matches!(
            term,
            MirTerminator::Call {
                callee_def_path: Some(path),
                ..
            } if path == "std::ptr::drop_in_place"
        )
}

/// Structural evidence for a local RAII panic guard.
///
/// This is deliberately independent of concrete library/type names. A guard is
/// recognized only when all of the following are visible in the canonical MIR:
/// 1. a local aggregate is constructed;
/// 2. that aggregate has a materialized custom `Drop` implementation;
/// 3. the `Drop` body writes through a pointer/reference stored in one of the
///    aggregate fields (restoration evidence);
/// 4. the same guard local is explicitly forgotten on a normal continuation;
/// 5. immediately before a potentially-panicking destructor, a different
///    projected field of the guard is updated (progress/commit evidence).
///
/// The owner allocation is not inferred from type text. It is recovered from
/// the identity domain through the captured restoration field at the current
/// program point.
#[derive(Clone, Debug, Default)]
struct PanicGuardEvidence {
    capture_subjects: BTreeMap<u32, String>,
    restore_fields: BTreeSet<u32>,
    forget_nodes: BTreeSet<String>,
}

fn normalize_type_identity(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .replace("::<", "<")
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut paren = 0i32;
    let mut square = 0i32;
    let mut brace = 0i32;
    let mut angle = 0i32;

    for (idx, ch) in text.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => square += 1,
            ']' => square -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            '<' => angle += 1,
            '>' => angle -= 1,
            ',' if paren == 0 && square == 0 && brace == 0 && angle == 0 => {
                out.push(text[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    if start <= text.len() {
        let tail = text[start..].trim();
        if !tail.is_empty() {
            out.push(tail);
        }
    }
    out
}

fn aggregate_guard_construction(
    function: &str,
    statement: &MirStatement,
) -> Option<(ProgramVarId, String, BTreeMap<u32, String>)> {
    if statement.kind != "Assign" {
        return None;
    }
    let place = statement.place.as_deref()?;
    if place.contains(" -> ") {
        return None;
    }
    let guard = ProgramVarId::rust(function, place)?;
    let rvalue = statement.rvalue.as_deref()?.trim();
    let open = rvalue.find(" { ")?;
    let aggregate_type = normalize_type_identity(&rvalue[..open]);
    let body = rvalue.get(open + 3..)?.strip_suffix('}')?;

    let mut fields = BTreeMap::new();
    for (index, field) in split_top_level_commas(body).into_iter().enumerate() {
        let operand = field
            .split_once(':')
            .map(|(_, rhs)| rhs.trim())
            .unwrap_or(field);
        if let Some(local) = parse_mir_local_index(operand) {
            fields.insert(index as u32, format!("Local(_{local})"));
        }
    }
    (!fields.is_empty()).then_some((guard, aggregate_type, fields))
}

fn first_field_index(place: &str) -> Option<u32> {
    let marker = "Field(";
    let start = place.find(marker)? + marker.len();
    let digits: String = place[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn projected_progress_write(
    function: &str,
    statement: &MirStatement,
) -> Option<(ProgramVarId, u32)> {
    if statement.kind != "Assign" {
        return None;
    }
    let place = statement.place.as_deref()?;
    if !place.contains(" -> Field(") || place.contains(" -> *") {
        return None;
    }
    // A progress commit is data-dependent, not a zero-operand aggregate or a
    // reference creation. This excludes many unrelated field initializations.
    let rvalue = statement.rvalue.as_deref()?.trim();
    if parse_mir_local_index(rvalue).is_none() || rvalue.starts_with('&') || rvalue.contains(" { ") {
        return None;
    }
    Some((ProgramVarId::rust(function, place)?, first_field_index(place)?))
}

fn drop_impl_owner_type(function: &str) -> Option<String> {
    let rest = function.strip_prefix('<')?;
    let (owner, suffix) = rest.split_once(" as ")?;
    if !suffix.contains("Drop>::drop") {
        return None;
    }
    Some(normalize_type_identity(owner))
}

fn restoration_field_from_drop_write(place: &str) -> Option<u32> {
    if parse_mir_local_index(place)? != 1 {
        return None;
    }
    // `*self.field = ...` contains the dereference of `self` and then the
    // dereference of the captured pointer/reference. Requiring two dereference
    // projections avoids treating an ordinary update to guard metadata as a
    // restoration write.
    if place.match_indices(" -> *").count() < 2 {
        return None;
    }
    first_field_index(place)
}

fn mem_forget_subject(term: &MirTerminator) -> Option<&str> {
    let MirTerminator::Call {
        callee_def_path: Some(path),
        arguments,
        ..
    } = term
    else {
        return None;
    };
    if !(path.contains("std::mem::forget") || path.contains("core::mem::forget")) {
        return None;
    }
    arguments.first().map(|arg| arg.arg.as_str())
}

fn collect_panic_guard_evidence(icfg: &GlobalICFGOrdered) -> BTreeMap<ProgramVarId, PanicGuardEvidence> {
    let mut constructions: BTreeMap<ProgramVarId, (String, BTreeMap<u32, String>)> = BTreeMap::new();
    let mut forget_nodes: BTreeMap<ProgramVarId, BTreeSet<String>> = BTreeMap::new();
    let mut restore_fields_by_type: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(block) = node else {
            continue;
        };
        let Some(function) = mir_function_for_node(node_id) else {
            continue;
        };

        for statement in &block.statements {
            if let Some((guard, aggregate_type, fields)) =
                aggregate_guard_construction(&function, statement)
            {
                constructions.insert(guard, (aggregate_type, fields));
            }
        }

        if let Some(term) = block.terminator.as_ref() {
            if let Some(subject) = mem_forget_subject(term) {
                if let Some(guard) = ProgramVarId::rust(function.clone(), subject) {
                    forget_nodes.entry(guard).or_default().insert(node_id.clone());
                }
            }
        }

        if let Some(owner_type) = drop_impl_owner_type(&function) {
            for statement in &block.statements {
                if statement.kind != "Assign" {
                    continue;
                }
                let Some(place) = statement.place.as_deref() else {
                    continue;
                };
                if let Some(field) = restoration_field_from_drop_write(place) {
                    restore_fields_by_type
                        .entry(owner_type.clone())
                        .or_default()
                        .insert(field);
                }
            }
        }
    }

    let mut out = BTreeMap::new();
    for (guard, (aggregate_type, capture_subjects)) in constructions {
        let Some(forget_nodes) = forget_nodes.get(&guard) else {
            continue;
        };
        let Some(restore_fields) = restore_fields_by_type.get(&aggregate_type) else {
            continue;
        };
        if restore_fields.is_empty() {
            continue;
        }
        out.insert(
            guard,
            PanicGuardEvidence {
                capture_subjects,
                restore_fields: restore_fields.clone(),
                forget_nodes: forget_nodes.clone(),
            },
        );
    }
    out
}

fn normal_reachable_to_any(
    icfg: &GlobalICFGOrdered,
    start: &str,
    targets: &BTreeSet<String>,
) -> bool {
    if targets.contains(start) {
        return true;
    }
    let mut seen = BTreeSet::new();
    let mut work = VecDeque::from([start.to_string()]);
    while let Some(node) = work.pop_front() {
        if !seen.insert(node.clone()) {
            continue;
        }
        for edge in icfg.icfg_edges.iter().filter(|edge| edge.source == node) {
            if edge_flow_kind(edge) != EdgeFlowKind::Normal {
                continue;
            }
            if targets.contains(&edge.destination) {
                return true;
            }
            if !seen.contains(&edge.destination) {
                work.push_back(edge.destination.clone());
            }
        }
    }
    false
}

#[derive(Clone, Debug)]
struct GuardCommitObservation {
    guard: ProgramVarId,
    progress_field: u32,
    keys: BTreeSet<String>,
    used_owner_fallback: bool,
}

fn guard_commit_observations(
    icfg: &GlobalICFGOrdered,
    node_id: &str,
    block: &crate::structs::MirBasicBlock,
    identity: &AllocationIdentityState,
    input: &PanicLifecycleMemory,
    guards: &BTreeMap<ProgramVarId, PanicGuardEvidence>,
) -> Vec<GuardCommitObservation> {
    let Some(function) = mir_function_for_node(node_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    for statement in &block.statements {
        let Some((guard, progress_field)) = projected_progress_write(&function, statement) else {
            continue;
        };
        let Some(evidence) = guards.get(&guard) else {
            continue;
        };
        if evidence.restore_fields.contains(&progress_field)
            || !normal_reachable_to_any(icfg, node_id, &evidence.forget_nodes)
        {
            continue;
        }

        let mut keys = BTreeSet::new();
        let mut used_owner_fallback = false;
        for restore_field in &evidence.restore_fields {
            let Some(subject) = evidence.capture_subjects.get(restore_field) else {
                continue;
            };
            let resolution = lifecycle_allocations_for_subject(node_id, subject, identity);
            used_owner_fallback |= resolution.owner_scoped_fallback;
            for allocation in resolution.allocations {
                let key = lifecycle_key_for_allocation(&allocation);
                if input.get(&key).may_stale_owner() {
                    keys.insert(key);
                }
            }
        }

        if !keys.is_empty() {
            out.push(GuardCommitObservation {
                guard,
                progress_field,
                keys,
                used_owner_fallback,
            });
        }
    }
    out
}

/// Real A3 lifecycle producer over the canonical ICFG.
///
/// Producer semantics:
/// - normal return from `ManuallyDrop::take` => the referenced owner may now be
///   stale if panic occurs before a later commit;
/// - a structurally certified RAII panic guard may commit owner metadata before
///   a potentially-panicking destructor;
/// - unwind from a MIR Drop or `ptr::drop_in_place` => every still-stale owner
///   records a partial-drop MAY fact;
/// - all other edges are pure propagation in this satellite domain.
///
/// Commit recognition is intentionally proof-oriented: it requires aggregate
/// capture evidence, a materialized restoring `Drop`, a projected progress
/// write before the destructor, and a normally reachable `mem::forget`. Type
/// names and benchmark paths are not semantic inputs.
pub fn fixed_point_real_panic_lifecycle(
    icfg: &GlobalICFGOrdered,
    identity: &AllocationIdentityState,
    entry: &str,
) -> Result<PanicLifecycleState, String> {
    let nodes: BTreeMap<&str, &GlobalICFGNode> = icfg
        .ordered_nodes
        .iter()
        .map(|(id, node)| (id.as_str(), node))
        .collect();
    let panic_guards = collect_panic_guard_evidence(icfg);

    fixed_point_lifecycle_with_transfer(
        icfg,
        entry,
        &PanicLifecycleMemory::default(),
        |edge, input| {
            let mut out = input.clone();
            let Some(GlobalICFGNode::Mir(block)) = nodes.get(edge.source.as_str()).copied() else {
                return out;
            };
            let Some(term) = block.terminator.as_ref() else {
                return out;
            };

            if edge_flow_kind(edge) == EdgeFlowKind::Normal && is_manually_drop_take(term) {
                if let MirTerminator::Call { arguments, .. } = term {
                    if let Some(subject) = arguments.first() {
                        let resolution = lifecycle_allocations_for_subject(
                            &edge.source,
                            &subject.arg,
                            identity,
                        );
                        if resolution.allocations.is_empty() {
                            out.mark_unresolved();
                            eprintln!(
                                "A3_PANIC_LIFECYCLE_EXTRACT_UNRESOLVED: node={} subject={}",
                                edge.source, subject.arg
                            );
                        } else if resolution.owner_scoped_fallback {
                            // A positive MAY fact is still useful, but failure to
                            // prove the exact field path must not be advertised as
                            // complete producer coverage.
                            out.mark_unresolved();
                            eprintln!(
                                "A3_PANIC_LIFECYCLE_EXTRACT_OWNER_MAY: node={} subject={} candidates={}",
                                edge.source,
                                subject.arg,
                                resolution.allocations.len()
                            );
                        }
                        for allocation in resolution.allocations {
                            let key = lifecycle_key_for_allocation(&allocation);
                            let was_stale = out.get(&key).may_stale_owner();
                            out.extract_before_commit(&key);
                            if !was_stale {
                                eprintln!(
                                    "A3_PANIC_LIFECYCLE_EXTRACT: node={} allocation={}",
                                    edge.source, key
                                );
                            }
                        }
                    }
                }
            }

            if is_destructor_like_call(term) {
                for observation in guard_commit_observations(
                    icfg,
                    &edge.source,
                    block,
                    identity,
                    &out,
                    &panic_guards,
                ) {
                    if observation.used_owner_fallback {
                        // Correlation stayed owner-scoped rather than exact; keep
                        // coverage fail-closed even though the commit transfer is
                        // structurally justified.
                        out.mark_unresolved();
                    }
                    for key in &observation.keys {
                        out.commit_before_drop(key);
                        if edge_flow_kind(edge) == EdgeFlowKind::Unwind {
                            // The element destructor did unwind after the metadata
                            // commit. Record the partial drop while keeping stale
                            // ownership cleared, which is exactly what prevents a
                            // repeat-drop witness.
                            out.drop_unwind(key);
                        }
                    }
                    if edge_flow_kind(edge) == EdgeFlowKind::Unwind {
                        eprintln!(
                            "A3_PANIC_LIFECYCLE_COMMIT_GUARD: node={} guard={} progress_field={} committed={}",
                            edge.source,
                            observation.guard.canonical_string(),
                            observation.progress_field,
                            observation.keys.len()
                        );
                    }
                }
            }

            if edge_flow_kind(edge) == EdgeFlowKind::Unwind && is_destructor_like_call(term) {
                let affected = out.mark_stale_owners_partial_drop();
                if affected > 0 {
                    eprintln!(
                        "A3_PANIC_LIFECYCLE_PARTIAL_UNWIND: node={} stale_owners={}",
                        edge.source, affected
                    );
                }
            }

            out
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "alloc#aligned_box-container";

    #[test]
    fn lifecycle_lattice_bottom_and_join_are_pointwise_may() {
        let bottom = LifecycleValue::bottom();
        let owned = LifecycleValue::owned();
        let committed = owned.commit_before_drop();
        let partial = owned.drop_unwind();

        assert!(bottom.leq(owned));
        assert!(owned.leq(committed));
        assert!(!committed.leq(owned));

        let joined = committed.join(partial);
        assert!(committed.leq(joined));
        assert!(partial.leq(joined));
        assert!(joined.may_own());
        assert!(joined.may_committed());
        assert!(joined.may_partial_drop());
    }

    #[test]
    fn vulnerable_extract_then_panic_preserves_stale_owner_witness() {
        let mut memory = PanicLifecycleMemory::default();
        memory.mark_owned(OWNER);
        memory.extract_before_commit(OWNER);
        memory.drop_unwind(OWNER);

        let value = memory.get(OWNER);
        assert!(value.may_own());
        assert!(value.may_partial_drop());
        assert!(value.may_stale_owner());
        assert!(!value.may_committed());
        assert!(value.may_repeat_drop());
    }

    #[test]
    fn fixed_commit_before_panicking_drop_clears_stale_owner_on_that_path() {
        let mut memory = PanicLifecycleMemory::default();
        memory.mark_owned(OWNER);
        memory.extract_before_commit(OWNER);
        memory.commit_before_drop(OWNER);
        memory.drop_unwind(OWNER);

        let value = memory.get(OWNER);
        assert!(value.may_own());
        assert!(value.may_partial_drop());
        assert!(value.may_committed());
        assert!(!value.may_stale_owner());
        assert!(!value.may_repeat_drop());
    }

    #[test]
    fn normal_drop_completion_consumes_repeat_drop_capability() {
        let mut memory = PanicLifecycleMemory::default();
        memory.mark_owned(OWNER);
        memory.extract_before_commit(OWNER);
        memory.drop_return(OWNER);

        let value = memory.get(OWNER);
        assert!(!value.may_own());
        assert!(!value.may_partial_drop());
        assert!(!value.may_stale_owner());
        assert!(value.may_complete());
        assert!(!value.may_repeat_drop());
    }

    #[test]
    fn join_of_fixed_and_vulnerable_paths_keeps_may_repeat_drop() {
        let mut vulnerable = PanicLifecycleMemory::default();
        vulnerable.mark_owned(OWNER);
        vulnerable.extract_before_commit(OWNER);
        vulnerable.drop_unwind(OWNER);

        let mut fixed = PanicLifecycleMemory::default();
        fixed.mark_owned(OWNER);
        fixed.extract_before_commit(OWNER);
        fixed.commit_before_drop(OWNER);
        fixed.drop_unwind(OWNER);

        let joined = vulnerable.join(&fixed);
        let value = joined.get(OWNER);
        assert!(value.may_stale_owner());
        assert!(value.may_committed());
        assert!(value.may_partial_drop());
        assert!(value.may_repeat_drop());
        assert!(vulnerable.leq(&joined));
        assert!(fixed.leq(&joined));
    }


    #[test]
    fn lifecycle_coverage_join_is_monotone_and_unresolved_dominates() {
        let complete = PanicLifecycleMemory::default();
        let mut unresolved = PanicLifecycleMemory::default();
        unresolved.mark_unresolved();

        assert!(complete.leq(&unresolved));
        assert!(!unresolved.leq(&complete));
        assert_eq!(complete.join(&unresolved).coverage(), LifecycleCoverage::Unresolved);
        assert_eq!(unresolved.join(&complete).coverage(), LifecycleCoverage::Unresolved);
    }

    #[test]
    fn program_point_state_join_is_monotone() {
        let mut state = PanicLifecycleState::default();

        let mut first = PanicLifecycleMemory::default();
        first.mark_owned(OWNER);
        assert!(state.join_into("rust::f::bb1", &first));
        assert!(!state.join_into("rust::f::bb1", &first));

        let mut second = first.clone();
        second.extract_before_commit(OWNER);
        second.drop_unwind(OWNER);
        assert!(state.join_into("rust::f::bb1", &second));
        assert!(state.get("rust::f::bb1").get(OWNER).may_repeat_drop());
    }

    fn terminal(reason: &str) -> crate::structs::GlobalICFGNode {
        crate::structs::GlobalICFGNode::Terminal(crate::structs::TerminalNode {
            reason: reason.to_string(),
        })
    }

    fn edge(source: &str, destination: &str, label: &str) -> IcfgEdge {
        IcfgEdge {
            source: source.to_string(),
            destination: destination.to_string(),
            label: Some(label.to_string()),
            source_label: None,
            destination_label: None,
        }
    }

    fn synthetic_icfg(nodes: &[&str], edges: Vec<IcfgEdge>) -> GlobalICFGOrdered {
        GlobalICFGOrdered {
            ordered_nodes: nodes
                .iter()
                .map(|id| (id.to_string(), terminal("lifecycle-test")))
                .collect(),
            icfg_edges: edges,
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),
        }
    }

    fn test_edge_transfer(edge: &IcfgEdge, input: &PanicLifecycleMemory) -> PanicLifecycleMemory {
        let mut out = input.clone();
        match edge.label.as_deref() {
            Some("test:extract") => out.extract_before_commit(OWNER),
            Some("test:commit") => out.commit_before_drop(OWNER),
            Some("test:drop_unwind") => out.drop_unwind(OWNER),
            Some("test:drop_return") => out.drop_return(OWNER),
            _ => {}
        }
        out
    }

    #[test]
    fn cfg_passthrough_propagates_existing_facts_to_reachable_successors() {
        let icfg = synthetic_icfg(
            &["entry", "mid", "exit"],
            vec![edge("entry", "mid", "Goto"), edge("mid", "exit", "Goto")],
        );
        let mut seed = PanicLifecycleMemory::default();
        seed.mark_owned(OWNER);
        seed.extract_before_commit(OWNER);

        let state = fixed_point_lifecycle_passthrough(&icfg, "entry", &seed).unwrap();
        assert!(state.get("mid").get(OWNER).may_stale_owner());
        assert!(state.get("exit").get(OWNER).may_stale_owner());
    }

    #[test]
    fn cfg_vulnerable_path_propagates_repeat_drop_witness_through_unwind() {
        let icfg = synthetic_icfg(
            &["entry", "extracted", "cleanup"],
            vec![
                edge("entry", "extracted", "test:extract"),
                edge("extracted", "cleanup", "test:drop_unwind"),
            ],
        );
        let mut seed = PanicLifecycleMemory::default();
        seed.mark_owned(OWNER);

        let state = fixed_point_lifecycle_with_transfer(&icfg, "entry", &seed, test_edge_transfer)
            .unwrap();
        let cleanup = state.get("cleanup").get(OWNER);
        assert!(cleanup.may_partial_drop());
        assert!(cleanup.may_stale_owner());
        assert!(cleanup.may_repeat_drop());
    }

    #[test]
    fn cfg_commit_before_unwind_suppresses_repeat_drop_on_that_path() {
        let icfg = synthetic_icfg(
            &["entry", "extracted", "committed", "cleanup"],
            vec![
                edge("entry", "extracted", "test:extract"),
                edge("extracted", "committed", "test:commit"),
                edge("committed", "cleanup", "test:drop_unwind"),
            ],
        );
        let mut seed = PanicLifecycleMemory::default();
        seed.mark_owned(OWNER);

        let state = fixed_point_lifecycle_with_transfer(&icfg, "entry", &seed, test_edge_transfer)
            .unwrap();
        let cleanup = state.get("cleanup").get(OWNER);
        assert!(cleanup.may_partial_drop());
        assert!(cleanup.may_committed());
        assert!(!cleanup.may_stale_owner());
        assert!(!cleanup.may_repeat_drop());
    }

    #[test]
    fn cfg_join_preserves_vulnerable_may_witness_against_committed_path() {
        let icfg = synthetic_icfg(
            &["entry", "v_extract", "v_cleanup", "f_extract", "f_commit", "f_cleanup", "join"],
            vec![
                edge("entry", "v_extract", "test:extract"),
                edge("v_extract", "v_cleanup", "test:drop_unwind"),
                edge("v_cleanup", "join", "Goto"),
                edge("entry", "f_extract", "test:extract"),
                edge("f_extract", "f_commit", "test:commit"),
                edge("f_commit", "f_cleanup", "test:drop_unwind"),
                edge("f_cleanup", "join", "Goto"),
            ],
        );
        let mut seed = PanicLifecycleMemory::default();
        seed.mark_owned(OWNER);

        let state = fixed_point_lifecycle_with_transfer(&icfg, "entry", &seed, test_edge_transfer)
            .unwrap();
        let joined = state.get("join").get(OWNER);
        assert!(joined.may_partial_drop());
        assert!(joined.may_committed());
        assert!(joined.may_stale_owner());
        assert!(joined.may_repeat_drop());
    }



    #[test]
    fn cfg_unresolved_coverage_propagates_only_downstream() {
        let icfg = synthetic_icfg(
            &["entry", "marked", "exit"],
            vec![edge("entry", "marked", "test:unresolved"), edge("marked", "exit", "Goto")],
        );
        let state = fixed_point_lifecycle_with_transfer(
            &icfg,
            "entry",
            &PanicLifecycleMemory::default(),
            |edge, input| {
                let mut out = input.clone();
                if edge.label.as_deref() == Some("test:unresolved") {
                    out.mark_unresolved();
                }
                out
            },
        )
        .unwrap();

        assert_eq!(state.get("entry").coverage(), LifecycleCoverage::Complete);
        assert_eq!(state.get("marked").coverage(), LifecycleCoverage::Unresolved);
        assert_eq!(state.get("exit").coverage(), LifecycleCoverage::Unresolved);
    }

    fn mir_node(block_id: usize, terminator: MirTerminator) -> GlobalICFGNode {
        GlobalICFGNode::Mir(crate::structs::MirBasicBlock {
            block_id,
            statements: Vec::new(),
            terminator: Some(terminator),
        })
    }

    fn mir_node_with_statements(
        block_id: usize,
        statements: Vec<MirStatement>,
        terminator: MirTerminator,
    ) -> GlobalICFGNode {
        GlobalICFGNode::Mir(crate::structs::MirBasicBlock {
            block_id,
            statements,
            terminator: Some(terminator),
        })
    }

    fn test_source_info() -> crate::structs::SourceInfoData {
        crate::structs::SourceInfoData {
            span: String::new(),
            scope: String::new(),
        }
    }

    fn assign_statement(place: &str, rvalue: &str) -> MirStatement {
        MirStatement {
            source_info: test_source_info(),
            kind: "Assign".to_string(),
            details: format!("Assign(({place}, {rvalue}))"),
            place: Some(place.to_string()),
            is_mutable: Some(true),
            rvalue: Some(rvalue.to_string()),
        }
    }

    fn goto_term(target: &str) -> MirTerminator {
        MirTerminator::Goto {
            details: String::new(),
            source_info: String::new(),
            target: target.to_string(),
        }
    }

    fn return_term() -> MirTerminator {
        MirTerminator::Return {
            details: String::new(),
            source_info: String::new(),
        }
    }

    fn call_term(callee: &str, arg: &str, return_target: &str, unwind_target: &str) -> MirTerminator {
        MirTerminator::Call {
            details: String::new(),
            source_info: String::new(),
            function_called: callee.to_string(),
            callee_def_path: Some(callee.to_string()),
            deallocator_evidence: None,
            allocation_disposition_evidence: None,
            higher_order_evidence: None,
            callee_is_local: false,
            callback_def_paths: Vec::new(),
            resolved_instance_callees: Vec::new(),
            instance_dispatch_observed: true,
            instance_dispatch_external: true,
            instance_dispatch_unresolved: false,
            arguments: vec![crate::structs::MirCallArgument {
                arg: arg.to_string(),
                is_mutable: Some(true),
            }],
            return_place: "_0".to_string(),
            return_target: Some(return_target.to_string()),
            unwind_target: unwind_target.to_string(),
        }
    }

    #[test]
    fn real_producer_extract_then_drop_in_place_unwind_creates_may_repeat_drop() {
        let take = "rust::test::f::bb0";
        let drop = "rust::test::f::bb1";
        let cleanup = "rust::test::f::bb2";
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![
                (
                    take.to_string(),
                    mir_node(
                        0,
                        call_term(
                            "std::mem::ManuallyDrop::<T>::take",
                            "Local(_1) [mutable]",
                            "bb1",
                            "continue",
                        ),
                    ),
                ),
                (
                    drop.to_string(),
                    mir_node(
                        1,
                        call_term(
                            "std::ptr::drop_in_place",
                            "Local(_2)",
                            "bb3",
                            "cleanup(bb2)",
                        ),
                    ),
                ),
                (cleanup.to_string(), terminal("cleanup")),
            ],
            icfg_edges: vec![
                edge(take, drop, "Resolved external Instance summary return"),
                edge(drop, cleanup, "Call unwind"),
            ],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),
        };

        let allocation = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "test::f".into(),
                label: "owner".into(),
            },
            Vec::new(),
        );
        let mut identity = AllocationIdentityState::default();
        let mut event = AllocationIdentityMemory::default();
        event.assign_points_to(
            ProgramVarId::rust("test::f", "Local(_1)").unwrap(),
            BTreeSet::from([allocation.clone()]),
        );
        identity.event_by_node.insert(take.to_string(), event);

        let state = fixed_point_real_panic_lifecycle(&icfg, &identity, take).unwrap();
        let key = lifecycle_key_for_allocation(&allocation);
        let value = state.get(cleanup).get(&key);
        assert!(value.may_own());
        assert!(value.may_stale_owner());
        assert!(value.may_partial_drop());
        assert!(value.may_repeat_drop());
    }

    #[test]
    fn real_producer_does_not_invent_owner_when_take_identity_is_missing() {
        let take = "rust::test::f::bb0";
        let exit = "rust::test::f::bb1";
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![
                (
                    take.to_string(),
                    mir_node(
                        0,
                        call_term(
                            "std::mem::ManuallyDrop::<T>::take",
                            "Local(_1) [mutable]",
                            "bb1",
                            "continue",
                        ),
                    ),
                ),
                (exit.to_string(), terminal("exit")),
            ],
            icfg_edges: vec![edge(take, exit, "Resolved external Instance summary return")],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),
        };
        let identity = AllocationIdentityState::default();
        let state = fixed_point_real_panic_lifecycle(&icfg, &identity, take).unwrap();
        let exit_state = state.get(exit);
        assert_eq!(exit_state.iter().count(), 0);
        assert_eq!(exit_state.coverage(), LifecycleCoverage::Unresolved);
    }


    #[test]
    fn lifecycle_subject_resolution_follows_stack_reference_into_projected_owner_field() {
        let node = "rust::test::f::bb0";
        let allocation = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "test::f".into(),
                label: "field-owner".into(),
            },
            Vec::new(),
        );
        let reference = ProgramVarId::rust("test::f", "Local(_1)").unwrap();
        let owner = ProgramVarId::rust("test::f", "Local(_9)").unwrap();
        let owner_place = PlaceId {
            base: owner.clone(),
            projection: Vec::new(),
        };
        let field_place = PlaceId {
            base: owner,
            projection: vec![crate::structs::PlaceProjection::Field { index: 0 }],
        };

        let mut memory = AllocationIdentityMemory::default();
        memory.assign_stack_refs(reference, BTreeSet::from([owner_place]));
        memory.assign_place_points_to(field_place, BTreeSet::from([allocation.clone()]));
        let mut identity = AllocationIdentityState::default();
        identity.event_by_node.insert(node.to_string(), memory);

        assert_eq!(
            lifecycle_allocations_for_subject(node, "Local(_1) [mutable]", &identity),
            LifecycleSubjectResolution {
                allocations: vec![allocation],
                owner_scoped_fallback: false,
            }
        );
    }

    #[test]
    fn lifecycle_subject_resolution_owner_scoped_fallback_stays_with_proven_owner() {
        let node = "rust::test::f::bb0";
        let wanted = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "caller".into(),
                label: "wanted-owner".into(),
            },
            Vec::new(),
        );
        let unrelated = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "caller".into(),
                label: "unrelated-owner".into(),
            },
            Vec::new(),
        );

        let subject = ProgramVarId::rust("test::f", "Local(_27)").unwrap();
        let callee_self = ProgramVarId::rust("test::f", "Local(_1)").unwrap();
        let caller_owner = ProgramVarId::rust("caller", "Local(_9)").unwrap();
        let other_owner = ProgramVarId::rust("caller", "Local(_8)").unwrap();

        let mut memory = AllocationIdentityMemory::default();
        memory.assign_stack_refs(
            subject,
            BTreeSet::from([PlaceId {
                base: callee_self.clone(),
                projection: vec![crate::structs::PlaceProjection::Field { index: 0 }],
            }]),
        );
        memory.assign_stack_refs(
            callee_self,
            BTreeSet::from([PlaceId {
                base: caller_owner.clone(),
                projection: Vec::new(),
            }]),
        );

        // The exact rebased field is intentionally absent.  The represented
        // allocation is nevertheless below the same proven owner aggregate.
        memory.assign_place_points_to(
            PlaceId {
                base: caller_owner,
                projection: vec![
                    crate::structs::PlaceProjection::Field { index: 1 },
                    crate::structs::PlaceProjection::Field { index: 0 },
                ],
            },
            BTreeSet::from([wanted.clone()]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: other_owner,
                projection: vec![crate::structs::PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([unrelated]),
        );

        let mut identity = AllocationIdentityState::default();
        identity.event_by_node.insert(node.to_string(), memory);

        assert_eq!(
            lifecycle_allocations_for_subject(node, "Local(_27) [mutable]", &identity),
            LifecycleSubjectResolution {
                allocations: vec![wanted],
                owner_scoped_fallback: true,
            }
        );
    }

    fn panic_guard_test_icfg(include_forget: bool, restoring_drop: bool) -> GlobalICFGOrdered {
        let take = "rust::test::f::bb0";
        let init = "rust::test::f::bb1";
        let drop = "rust::test::f::bb2";
        let cleanup = "rust::test::f::bb3";
        let forget = "rust::test::f::bb4";
        let exit = "rust::test::f::bb5";
        let guard_drop = "rust::<test::Guard as std::ops::Drop>::drop::bb0";

        let mut nodes = vec![
            (
                take.to_string(),
                mir_node(
                    0,
                    call_term(
                        "std::mem::ManuallyDrop::<T>::take",
                        "Local(_27) [mutable]",
                        "bb1",
                        "continue",
                    ),
                ),
            ),
            (
                init.to_string(),
                mir_node_with_statements(
                    1,
                    vec![assign_statement(
                        "Local(_30) [mutable]",
                        "test::Guard { owner: copy _32, progress: copy _7 }",
                    )],
                    goto_term("bb2"),
                ),
            ),
            (
                drop.to_string(),
                mir_node_with_statements(
                    2,
                    vec![assign_statement(
                        "Local(_30) [mutable] -> Field(1, Type: usize)",
                        "copy _7",
                    )],
                    call_term(
                        "std::ptr::drop_in_place",
                        "Local(_2)",
                        "bb4",
                        "cleanup(bb3)",
                    ),
                ),
            ),
            (cleanup.to_string(), terminal("cleanup")),
            (exit.to_string(), terminal("exit")),
        ];

        if include_forget {
            nodes.push((
                forget.to_string(),
                mir_node(
                    4,
                    call_term(
                        "std::mem::forget::<test::Guard>",
                        "Local(_30)",
                        "bb5",
                        "continue",
                    ),
                ),
            ));
        } else {
            nodes.push((forget.to_string(), mir_node(4, goto_term("bb5"))));
        }

        let restore_place = if restoring_drop {
            "Local(_1) [mutable] -> * -> Field(0, Type: *mut u8) -> *"
        } else {
            "Local(_1) [mutable] -> * -> Field(1, Type: usize)"
        };
        nodes.push((
            guard_drop.to_string(),
            mir_node_with_statements(
                0,
                vec![assign_statement(restore_place, "move _9")],
                return_term(),
            ),
        ));

        GlobalICFGOrdered {
            ordered_nodes: nodes,
            icfg_edges: vec![
                edge(take, init, "Resolved external Instance summary return"),
                edge(init, drop, "Goto"),
                edge(drop, forget, "Resolved external Instance summary return"),
                edge(drop, cleanup, "Call unwind"),
                edge(forget, exit, "Resolved external Instance summary return"),
            ],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),
        }
    }

    fn panic_guard_test_identity() -> (AllocationIdentityState, AbstractAllocId) {
        let allocation = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "test::f".into(),
                label: "guarded-owner".into(),
            },
            Vec::new(),
        );
        let mut identity = AllocationIdentityState::default();

        let mut take_event = AllocationIdentityMemory::default();
        take_event.assign_points_to(
            ProgramVarId::rust("test::f", "Local(_27)").unwrap(),
            BTreeSet::from([allocation.clone()]),
        );
        identity
            .event_by_node
            .insert("rust::test::f::bb0".to_string(), take_event);

        let mut drop_event = AllocationIdentityMemory::default();
        drop_event.assign_points_to(
            ProgramVarId::rust("test::f", "Local(_32)").unwrap(),
            BTreeSet::from([allocation.clone()]),
        );
        identity
            .event_by_node
            .insert("rust::test::f::bb2".to_string(), drop_event);

        (identity, allocation)
    }

    #[test]
    fn real_producer_raii_progress_guard_commits_before_panicking_drop() {
        let icfg = panic_guard_test_icfg(true, true);
        let (identity, allocation) = panic_guard_test_identity();
        let state =
            fixed_point_real_panic_lifecycle(&icfg, &identity, "rust::test::f::bb0").unwrap();
        let value = state
            .get("rust::test::f::bb3")
            .get(&lifecycle_key_for_allocation(&allocation));

        assert!(value.may_own());
        assert!(value.may_committed());
        assert!(value.may_partial_drop());
        assert!(!value.may_stale_owner());
        assert!(!value.may_repeat_drop());
    }

    #[test]
    fn real_producer_progress_write_without_normal_forget_is_not_a_commit() {
        let icfg = panic_guard_test_icfg(false, true);
        let (identity, allocation) = panic_guard_test_identity();
        let state =
            fixed_point_real_panic_lifecycle(&icfg, &identity, "rust::test::f::bb0").unwrap();
        let value = state
            .get("rust::test::f::bb3")
            .get(&lifecycle_key_for_allocation(&allocation));
        assert!(value.may_repeat_drop());
        assert!(!value.may_committed());
    }

    #[test]
    fn real_producer_progress_write_without_restoring_drop_is_not_a_commit() {
        let icfg = panic_guard_test_icfg(true, false);
        let (identity, allocation) = panic_guard_test_identity();
        let state =
            fixed_point_real_panic_lifecycle(&icfg, &identity, "rust::test::f::bb0").unwrap();
        let value = state
            .get("rust::test::f::bb3")
            .get(&lifecycle_key_for_allocation(&allocation));
        assert!(value.may_repeat_drop());
        assert!(!value.may_committed());
    }

    #[test]
    fn real_producer_owner_scoped_fallback_creates_repeat_drop_may_but_keeps_coverage_unresolved() {
        let take = "rust::test::f::bb0";
        let drop = "rust::test::f::bb1";
        let cleanup = "rust::test::f::bb2";
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![
                (
                    take.to_string(),
                    mir_node(
                        0,
                        call_term(
                            "std::mem::ManuallyDrop::<T>::take",
                            "Local(_27) [mutable]",
                            "bb1",
                            "continue",
                        ),
                    ),
                ),
                (
                    drop.to_string(),
                    mir_node(
                        1,
                        call_term(
                            "std::ptr::drop_in_place",
                            "Local(_2)",
                            "bb3",
                            "cleanup(bb2)",
                        ),
                    ),
                ),
                (cleanup.to_string(), terminal("cleanup")),
            ],
            icfg_edges: vec![
                edge(take, drop, "Resolved external Instance summary return"),
                edge(drop, cleanup, "Call unwind"),
            ],
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),
        };

        let allocation = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "caller".into(),
                label: "owner".into(),
            },
            Vec::new(),
        );
        let subject = ProgramVarId::rust("test::f", "Local(_27)").unwrap();
        let callee_self = ProgramVarId::rust("test::f", "Local(_1)").unwrap();
        let caller_owner = ProgramVarId::rust("caller", "Local(_9)").unwrap();
        let mut event = AllocationIdentityMemory::default();
        event.assign_stack_refs(
            subject,
            BTreeSet::from([PlaceId {
                base: callee_self.clone(),
                projection: vec![crate::structs::PlaceProjection::Field { index: 0 }],
            }]),
        );
        event.assign_stack_refs(
            callee_self,
            BTreeSet::from([PlaceId {
                base: caller_owner.clone(),
                projection: Vec::new(),
            }]),
        );
        event.assign_place_points_to(
            PlaceId {
                base: caller_owner,
                projection: vec![
                    crate::structs::PlaceProjection::Field { index: 1 },
                    crate::structs::PlaceProjection::Field { index: 0 },
                ],
            },
            BTreeSet::from([allocation.clone()]),
        );
        let mut identity = AllocationIdentityState::default();
        identity.event_by_node.insert(take.to_string(), event);

        let state = fixed_point_real_panic_lifecycle(&icfg, &identity, take).unwrap();
        let cleanup_state = state.get(cleanup);
        let value = cleanup_state.get(&lifecycle_key_for_allocation(&allocation));
        assert!(value.may_repeat_drop());
        assert_eq!(cleanup_state.coverage(), LifecycleCoverage::Unresolved);
    }

    #[test]
    fn owner_scoped_fallback_does_not_widen_to_unrelated_allocations() {
        let node = "rust::test::f::bb0";
        let unrelated = AbstractAllocId::new(
            crate::structs::AllocationSiteId::Synthetic {
                scope: "caller".into(),
                label: "unrelated".into(),
            },
            Vec::new(),
        );
        let subject = ProgramVarId::rust("test::f", "Local(_27)").unwrap();
        let owner = ProgramVarId::rust("caller", "Local(_9)").unwrap();
        let other = ProgramVarId::rust("caller", "Local(_8)").unwrap();
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_stack_refs(
            subject,
            BTreeSet::from([PlaceId {
                base: owner,
                projection: vec![crate::structs::PlaceProjection::Field { index: 0 }],
            }]),
        );
        memory.assign_place_points_to(
            PlaceId {
                base: other,
                projection: vec![crate::structs::PlaceProjection::Field { index: 0 }],
            },
            BTreeSet::from([unrelated]),
        );
        let mut identity = AllocationIdentityState::default();
        identity.event_by_node.insert(node.to_string(), memory);

        assert_eq!(
            lifecycle_allocations_for_subject(node, "Local(_27) [mutable]", &identity),
            LifecycleSubjectResolution::default()
        );
    }

}
