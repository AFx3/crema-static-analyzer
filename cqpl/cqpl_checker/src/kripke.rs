use crate::ast::{LabelPredicate, MayPredicate, StructuralLabelKind};
use crate::truth::Truth;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CellValue {
    Bottom,
    Boxtimes,
    Alloc,
    Freed,
    Mb,
    Immb,
    Mv,
    Top,
}

impl CellValue {
    /// Exact Phase-5 CellValue order:
    /// BOTTOM <= all; ALLOC <= MB/IMMB/MV; all <= TOP.
    pub fn leq(self, other: Self) -> bool {
        use CellValue::*;
        match (self, other) {
            (x, y) if x == y => true,
            (Bottom, _) => true,
            (_, Top) => true,
            (Alloc, Mb | Immb | Mv) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgramLanguage {
    Rust,
    C,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramVariable {
    /// Globally unique cross-language identifier used in CQPL environments.
    pub id: String,
    pub language: ProgramLanguage,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Alloc,
    Drop,
    Read,
    Write,
    Use,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLabel {
    pub predicate: EventKind,
    pub variable: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationEventCertainty {
    /// Schema-v2 allocation-event facts are derived only from MAY identity
    /// information.  A singleton abstract target is not a concrete MUST fact.
    MayAbstract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationContract {
    pub family: String,
    pub operation: String,
    pub language: String,
    /// allocation_contracts_v2 proof basis for deallocator contracts.
    /// The checker validates this closed vocabulary; it never derives family
    /// from diagnostic provenance strings.
    #[serde(default)]
    pub basis: Option<String>,
    #[serde(default)]
    pub owner_def_path: Option<String>,
    #[serde(default)]
    pub allocator_def_path: Option<String>,
    #[serde(default)]
    pub callee_def_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationEventLabel {
    pub predicate: EventKind,
    pub allocation: String,
    pub certainty: AllocationEventCertainty,
    #[serde(default)]
    pub deallocator_contract: Option<AllocationContract>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationDispositionKind {
    BoxIntoRaw,
    BoxFromRaw,
    BoxLeak,
    MemForgetOwnedBox,
    RawPointerDropNoop,
    ReturnEscape,
    MayDeallocate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationObligationEffect {
    PreserveManualObligation,
    RestoreRaiiObligation,
    PreservePersistentObligation,
    PreserveUnreclaimedObligation,
    NoPointeeLifecycleEffect,
    MayEscapeToCaller,
    MayDischarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationDispositionRecord {
    pub allocation: String,
    pub kind: AllocationDispositionKind,
    pub certainty: AllocationEventCertainty,
    pub obligation_effect: AllocationObligationEffect,
    pub basis: String,
    #[serde(default)]
    pub source_variable: Option<String>,
    #[serde(default)]
    pub target_variable: Option<String>,
    #[serde(default)]
    pub callee_def_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbstractAllocation {
    pub id: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub site: Option<serde_json::Value>,
    #[serde(default)]
    pub context: Vec<String>,
    /// Present when `allocation_contracts_v1` is declared by the artifact.
    #[serde(default)]
    pub allocator_contract: Option<AllocationContract>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityPointsToRecord {
    pub variable: String,
    #[serde(default)]
    pub allocations: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentityAnnotation {
    #[serde(default)]
    pub points_to: Vec<IdentityPointsToRecord>,
    // Stack-reference records are exported for auditability by CREMA v2 but
    // are not needed by the checker after allocation_labels are materialized.
    #[serde(default)]
    pub stack_refs: Vec<serde_json::Value>,
}

/// One implementation-level AbstractMemory component. All variables in
/// `aliases` denote the same abstract allocation at this program point and
/// therefore share one CellValue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractCell {
    pub aliases: Vec<String>,
    pub value: CellValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractMemoryAnnotation {
    #[serde(default)]
    pub cells: Vec<AbstractCell>,
}

impl AbstractMemoryAnnotation {
    pub fn value_of(&self, var: &str) -> CellValue {
        self.cells
            .iter()
            .find(|cell| cell.aliases.iter().any(|v| v == var))
            .map(|cell| cell.value)
            .unwrap_or(CellValue::Bottom)
    }

    pub fn aliases_of(&self, var: &str) -> BTreeSet<String> {
        self.cells
            .iter()
            .find(|cell| cell.aliases.iter().any(|v| v == var))
            .map(|cell| cell.aliases.iter().cloned().collect())
            .unwrap_or_else(|| BTreeSet::from([var.to_string()]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractAllocationCell {
    pub allocation: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractAllocationMemoryAnnotation {
    #[serde(default)]
    pub cells: Vec<AbstractAllocationCell>,
}

impl AbstractAllocationMemoryAnnotation {
    pub fn value_of(&self, allocation: &str) -> CellValue {
        self.cells
            .iter()
            .find(|cell| cell.allocation == allocation)
            .map(|cell| cell.value)
            .unwrap_or(CellValue::Bottom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedNode {
    pub id: String,
    #[serde(default)]
    pub successors: Vec<String>,
    #[serde(default)]
    pub labels: Vec<EventLabel>,
    /// v6P block-level structural MIR labels (stmt:/rvalue:/term:).
    #[serde(default)]
    pub semantic_labels: Vec<String>,
    #[serde(default)]
    pub allocation_labels: Vec<AllocationEventLabel>,
    #[serde(default)]
    pub allocation_disposition: Vec<AllocationDispositionRecord>,
    #[serde(default)]
    pub identity: Option<NodeIdentityAnnotation>,
    #[serde(default)]
    pub event_identity: Option<NodeIdentityAnnotation>,
    #[serde(default)]
    pub allocation_post: Option<AbstractAllocationMemoryAnnotation>,
    #[serde(default)]
    pub pre: AbstractMemoryAnnotation,
    #[serde(default)]
    pub post: AbstractMemoryAnnotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedIcfg {
    pub schema_version: u32,
    pub entry: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Legacy/program-variable quantifier domain. It intentionally includes Rust and C variables.
    pub variables: Vec<ProgramVariable>,
    /// Canonical abstract-allocation quantifier domain introduced by schema v2.
    #[serde(default)]
    pub allocations: Vec<AbstractAllocation>,
    pub nodes: Vec<AnnotatedNode>,
}

#[derive(Debug, Clone)]
pub struct Kripke {
    pub schema_version: u32,
    pub entry: String,
    pub capabilities: BTreeSet<String>,
    pub variables: BTreeMap<String, ProgramVariable>,
    pub allocations: BTreeMap<String, AbstractAllocation>,
    pub nodes: BTreeMap<String, AnnotatedNode>,
}

fn rust_function_scope(node_id: &str) -> Option<&str> {
    if let Some((scope, bb)) = node_id.rsplit_once("::bb") {
        if scope.starts_with("rust::") && !bb.is_empty() && bb.chars().all(|c| c.is_ascii_digit()) {
            return Some(scope);
        }
    }
    // v6K.4 explicit unwind-terminal nodes are part of the same Rust
    // function's intra projection and represent a maximal path endpoint.
    if let Some(scope) = node_id.strip_suffix("::terminate") {
        if scope.starts_with("rust::") {
            return Some(scope);
        }
    }
    None
}

fn valid_structural_label(label: &str) -> bool {
    let Some((kind, name)) = label.split_once(':') else { return false; };
    if !matches!(kind, "stmt" | "rvalue" | "term") || name.is_empty() {
        return false;
    }
    name.chars().all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn validate_allocation_disposition_record(record: &AllocationDispositionRecord) -> Result<(), String> {
    use AllocationDispositionKind as K;
    use AllocationObligationEffect as E;

    let expected = match record.kind {
        K::BoxIntoRaw => (E::PreserveManualObligation, "rustc_box_into_raw_v1", true),
        K::BoxFromRaw => (E::RestoreRaiiObligation, "rustc_box_from_raw_v1", true),
        K::BoxLeak => (E::PreservePersistentObligation, "rustc_box_leak_v1", true),
        K::MemForgetOwnedBox => (E::PreserveUnreclaimedObligation, "rustc_mem_forget_owned_box_v1", true),
        K::RawPointerDropNoop => (E::NoPointeeLifecycleEffect, "rustc_mem_drop_raw_pointer_v1", true),
        K::ReturnEscape => (E::MayEscapeToCaller, "rust_return_identity_v1", false),
        K::MayDeallocate => (E::MayDischarge, "allocation_drop_label_v1", false),
    };
    if record.obligation_effect != expected.0 || record.basis != expected.1 {
        return Err(format!(
            "allocation_disposition_v1 invalid tuple for {:?}: effect={:?} basis={}",
            record.kind, record.obligation_effect, record.basis
        ));
    }
    if expected.2 && record.callee_def_path.as_deref().map_or(true, str::is_empty) {
        return Err(format!(
            "allocation_disposition_v1 {:?} requires producer callee_def_path provenance",
            record.kind
        ));
    }
    if !expected.2 && record.callee_def_path.is_some() {
        return Err(format!(
            "allocation_disposition_v1 {:?} must not invent call provenance",
            record.kind
        ));
    }
    Ok(())
}

impl Kripke {
    pub fn from_annotated_icfg(input: AnnotatedIcfg) -> Result<Self, String> {
        if !matches!(input.schema_version, 1 | 2) {
            return Err(format!(
                "unsupported annotated ICFG schema_version {}; expected 1 or 2",
                input.schema_version
            ));
        }
        let schema_version = input.schema_version;
        let capabilities: BTreeSet<String> = input.capabilities.iter().cloned().collect();
        let has_allocation_contracts = capabilities.contains("allocation_contracts_v1");
        let has_allocation_contracts_v2 = capabilities.contains("allocation_contracts_v2");
        let has_allocation_state = capabilities.contains("allocation_state_v1");
        let has_allocation_disposition = capabilities.contains("allocation_disposition_v1");
        if has_allocation_contracts && schema_version != 2 {
            return Err("allocation_contracts_v1 requires annotated ICFG schema v2".into());
        }
        if has_allocation_contracts_v2 && schema_version != 2 {
            return Err("allocation_contracts_v2 requires annotated ICFG schema v2".into());
        }
        if has_allocation_contracts_v2 && !has_allocation_contracts {
            return Err("allocation_contracts_v2 is a refinement of allocation_contracts_v1 and requires both artifact capabilities".into());
        }
        if has_allocation_state && schema_version != 2 {
            return Err("allocation_state_v1 requires annotated ICFG schema v2".into());
        }
        if has_allocation_disposition && schema_version != 2 {
            return Err("allocation_disposition_v1 requires annotated ICFG schema v2".into());
        }
        let has_mir_semantic_labels = capabilities.contains("mir_semantic_labels_v1");
        if has_mir_semantic_labels && schema_version != 2 {
            return Err("mir_semantic_labels_v1 requires annotated ICFG schema v2".into());
        }
        if capabilities.contains("mir_semantics_v2") && !has_mir_semantic_labels {
            return Err("mir_semantics_v2 requires mir_semantic_labels_v1 so the active transfer profile is auditable".into());
        }

        let mut variables = BTreeMap::new();
        for v in input.variables {
            if variables.insert(v.id.clone(), v).is_some() {
                return Err("duplicate program-variable id in annotated ICFG".into());
            }
        }
        if variables.is_empty() {
            return Err("annotated ICFG contains no program variables".into());
        }

        let mut allocations = BTreeMap::new();
        for allocation in input.allocations {
            if has_allocation_contracts && allocation.allocator_contract.is_none() {
                return Err(format!(
                    "artifact declares allocation_contracts_v1 but allocation '{}' is missing allocator_contract",
                    allocation.id
                ));
            }
            if let Some(contract) = allocation.allocator_contract.as_ref() {
                validate_contract(contract, "allocator_contract")?;
            }
            if allocation.id.is_empty() {
                return Err("empty abstract-allocation id in annotated ICFG".into());
            }
            if allocations.insert(allocation.id.clone(), allocation).is_some() {
                return Err("duplicate abstract-allocation id in annotated ICFG".into());
            }
        }
        let mut nodes = BTreeMap::new();
        for n in input.nodes {
            if !has_mir_semantic_labels && !n.semantic_labels.is_empty() {
                return Err(format!("node '{}' contains structural MIR labels without capability mir_semantic_labels_v1", n.id));
            }
            for label in &n.semantic_labels {
                if !valid_structural_label(label) {
                    return Err(format!("node '{}' contains invalid structural MIR label '{}'", n.id, label));
                }
            }
            validate_memory(&n.id, "pre", &n.pre, &variables)?;
            validate_memory(&n.id, "post", &n.post, &variables)?;
            if has_allocation_state {
                let allocation_post = n.allocation_post.as_ref().ok_or_else(|| {
                    format!(
                        "artifact declares allocation_state_v1 but node '{}' is missing allocation_post",
                        n.id
                    )
                })?;
                validate_allocation_memory(&n.id, allocation_post, &allocations)?;
            } else if let Some(allocation_post) = n.allocation_post.as_ref() {
                validate_allocation_memory(&n.id, allocation_post, &allocations)?;
            }
            if nodes.insert(n.id.clone(), n).is_some() {
                return Err("duplicate node id in annotated ICFG".into());
            }
        }
        if !nodes.contains_key(&input.entry) {
            return Err(format!("entry node '{}' is not present", input.entry));
        }

        for node in nodes.values() {
            for succ in &node.successors {
                if !nodes.contains_key(succ) {
                    return Err(format!("node '{}' references missing successor '{}'", node.id, succ));
                }
            }
            for label in &node.labels {
                if !variables.contains_key(&label.variable) {
                    return Err(format!("node '{}' label references undeclared variable '{}'", node.id, label.variable));
                }
            }
            for label in &node.allocation_labels {
                if !allocations.contains_key(&label.allocation) {
                    return Err(format!(
                        "node '{}' allocation label references undeclared allocation '{}'",
                        node.id, label.allocation
                    ));
                }
                if has_allocation_contracts && label.predicate == EventKind::Drop
                    && label.deallocator_contract.is_none()
                {
                    return Err(format!(
                        "artifact declares allocation_contracts_v1 but node '{}' drop label for '{}' is missing deallocator_contract",
                        node.id, label.allocation
                    ));
                }
                if let Some(contract) = label.deallocator_contract.as_ref() {
                    validate_contract(contract, "deallocator_contract")?;
                    if has_allocation_contracts_v2 && label.predicate == EventKind::Drop {
                        validate_v2_deallocator_contract(contract, "deallocator_contract")?;
                    }
                }
            }
            if !has_allocation_disposition && !node.allocation_disposition.is_empty() {
                return Err(format!(
                    "node '{}' contains allocation disposition records without capability allocation_disposition_v1",
                    node.id
                ));
            }
            for record in &node.allocation_disposition {
                if !allocations.contains_key(&record.allocation) {
                    return Err(format!(
                        "node '{}' disposition record references undeclared allocation '{}'",
                        node.id, record.allocation
                    ));
                }
                validate_allocation_disposition_record(record)?;
                for variable in [record.source_variable.as_ref(), record.target_variable.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    if !variables.contains_key(variable) {
                        return Err(format!(
                            "node '{}' disposition record references undeclared variable '{}'",
                            node.id, variable
                        ));
                    }
                }
            }

            for (kind, identity) in [
                ("identity", node.identity.as_ref()),
                ("event_identity", node.event_identity.as_ref()),
            ] {
                if let Some(identity) = identity {
                    for relation in &identity.points_to {
                        if !variables.contains_key(&relation.variable) {
                            return Err(format!(
                                "node '{}' {kind} relation references undeclared variable '{}'",
                                node.id, relation.variable
                            ));
                        }
                        for allocation in &relation.allocations {
                            if !allocations.contains_key(allocation) {
                                return Err(format!(
                                    "node '{}' {kind} relation references undeclared allocation '{}'",
                                    node.id, allocation
                                ));
                            }
                        }
                    }
                }
            }
        }

        Ok(Self { schema_version, entry: input.entry, capabilities, variables, allocations, nodes })
    }

    /// Resolve a node id or Rust function name to exactly one Kripke entry.
    /// Ambiguous suffixes are rejected rather than resolved by map order.
    pub fn resolve_entry(&self, requested: &str) -> Result<String, String> {
        if self.nodes.contains_key(requested) {
            return Ok(requested.to_string());
        }

        let normalized = requested
            .strip_prefix("rust::")
            .unwrap_or(requested)
            .trim_end_matches("::bb0");
        let mut candidates = BTreeSet::new();
        for id in self.nodes.keys() {
            let Some(scope) = rust_function_scope(id) else { continue; };
            if !id.ends_with("::bb0") {
                continue;
            }
            let bare_scope = scope.strip_prefix("rust::").unwrap_or(scope);
            if bare_scope == normalized || bare_scope.ends_with(&format!("::{normalized}")) {
                candidates.insert(id.clone());
            }
        }

        match candidates.len() {
            0 => Err(format!("no Kripke node/function matches entry '{requested}'")),
            1 => Ok(candidates.into_iter().next().unwrap()),
            _ => Err(format!(
                "ambiguous Kripke entry '{requested}'; candidates: {}",
                candidates.into_iter().collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// Return a deterministic entry-scoped Kripke projection.
    ///
    /// Whole-program mode retains every node reachable from the requested
    /// entry.  `intra=true` additionally restricts traversal to MIR basic
    /// blocks of the entry's Rust function; an interprocedural edge is a hard
    /// boundary and no synthetic call bypass is introduced.  This is an
    /// explicit *scoped* model-checking mode, not a replacement for the v6I
    /// whole-program no-refutation theorem.
    pub fn project_from_entry(&self, requested: &str, intra: bool) -> Result<Self, String> {
        let entry = self.resolve_entry(requested)?;
        let intra_scope = if intra {
            Some(
                rust_function_scope(&entry)
                    .ok_or_else(|| format!("--intra requires a Rust MIR entry, got '{entry}'"))?
                    .to_string(),
            )
        } else {
            None
        };

        let mut retained = BTreeSet::new();
        let mut worklist = std::collections::VecDeque::new();
        retained.insert(entry.clone());
        worklist.push_back(entry.clone());

        while let Some(id) = worklist.pop_front() {
            let Some(node) = self.nodes.get(&id) else { continue; };
            let mut successors = node.successors.clone();
            successors.sort();
            successors.dedup();
            for succ in successors {
                if let Some(scope) = intra_scope.as_deref() {
                    if rust_function_scope(&succ) != Some(scope) {
                        continue;
                    }
                }
                if retained.insert(succ.clone()) {
                    worklist.push_back(succ);
                }
            }
        }

        let mut nodes = BTreeMap::new();
        for id in &retained {
            let mut node = self.nodes.get(id).unwrap().clone();
            node.successors.retain(|succ| retained.contains(succ));
            node.successors.sort();
            node.successors.dedup();
            nodes.insert(id.clone(), node);
        }

        let mut used_variables = BTreeSet::new();
        let mut used_allocations = BTreeSet::new();
        for node in nodes.values() {
            used_variables.extend(node.labels.iter().map(|l| l.variable.clone()));
            used_allocations.extend(node.allocation_labels.iter().map(|l| l.allocation.clone()));
            for memory in [&node.pre, &node.post] {
                for cell in &memory.cells {
                    used_variables.extend(cell.aliases.iter().cloned());
                }
            }
            for identity in [node.identity.as_ref(), node.event_identity.as_ref()].into_iter().flatten() {
                for relation in &identity.points_to {
                    used_variables.insert(relation.variable.clone());
                    used_allocations.extend(relation.allocations.iter().cloned());
                }
            }
        }

        let variables = self
            .variables
            .iter()
            .filter(|(id, _)| used_variables.contains(*id))
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        let allocations = self
            .allocations
            .iter()
            .filter(|(id, _)| used_allocations.contains(*id))
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();

        Ok(Self {
            schema_version: self.schema_version,
            entry,
            capabilities: self.capabilities.clone(),
            variables,
            allocations,
            nodes,
        })
    }

    pub fn variable_ids(&self) -> impl Iterator<Item = &String> { self.variables.keys() }
    pub fn allocation_ids(&self) -> impl Iterator<Item = &String> { self.allocations.keys() }

    /// Alias component at this concrete program point in the abstract Kripke.
    /// Labels are associated with execution of the block, so both pre and post
    /// alias evidence is conservatively visible to label matching.
    pub fn structural_label_hold(&self, node_id: &str, kind: StructuralLabelKind, name: &str) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let prefix = match kind {
            StructuralLabelKind::Statement => "stmt",
            StructuralLabelKind::Rvalue => "rvalue",
            StructuralLabelKind::Terminator => "term",
        };
        let expected = format!("{prefix}:{name}");
        if node.semantic_labels.iter().any(|label| label == &expected) { Truth::True } else { Truth::False }
    }

    pub fn aliases_at(&self, node_id: &str, var: &str) -> BTreeSet<String> {
        let Some(node) = self.nodes.get(node_id) else {
            return BTreeSet::from([var.to_string()]);
        };
        let mut aliases = node.pre.aliases_of(var);
        aliases.extend(node.post.aliases_of(var));
        aliases.insert(var.to_string());
        aliases
    }

    /// TaintMayHold from the theory. The implementation-level alias grouping is
    /// already encoded inside Pi#_post; no extra global alias closure is added.
    pub fn may_hold(&self, node_id: &str, var: &str, p: MayPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let atom = match p {
            MayPredicate::Alloc => CellValue::Alloc,
            MayPredicate::Drop => CellValue::Freed,
            MayPredicate::OwnForg => CellValue::Mv,
        };
        if atom.leq(node.post.value_of(var)) { Truth::Unknown } else { Truth::False }
    }

    /// Exact syntactic label predicate, lifted only through the MAY-alias
    /// component available at this node (pre/post), including Rust<->C aliases.
    pub fn label_hold(&self, node_id: &str, var: &str, p: LabelPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let aliases = self.aliases_at(node_id, var);
        let holds = node.labels.iter().any(|label| {
            if !aliases.contains(&label.variable) { return false; }
            match p {
                LabelPredicate::Alloc => label.predicate == EventKind::Alloc,
                LabelPredicate::Drop => label.predicate == EventKind::Drop,
                LabelPredicate::Read => label.predicate == EventKind::Read,
                LabelPredicate::Write => label.predicate == EventKind::Write,
                LabelPredicate::Use => matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write),
                LabelPredicate::AllocatorMismatch => false,
            }
        });
        if holds { Truth::True } else { Truth::False }
    }

    /// Allocation-centric MAY state predicate provided by allocation_state_v1.
    /// Positive MAY membership yields `unk`, exactly like ProgramVar state
    /// predicates; exclusion yields `ff`.
    pub fn allocation_may_hold(&self, node_id: &str, allocation: &str, p: MayPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let Some(post) = node.allocation_post.as_ref() else { return Truth::False; };
        let atom = match p {
            MayPredicate::Alloc => CellValue::Alloc,
            MayPredicate::Drop => CellValue::Freed,
            MayPredicate::OwnForg => CellValue::Mv,
        };
        if atom.leq(post.value_of(allocation)) { Truth::Unknown } else { Truth::False }
    }

    /// Allocation-centric event predicate introduced by schema v2.
    ///
    /// Schema v2 contains only `may_abstract` labels.  Matching a positive
    /// allocation-event atom therefore yields `unk`; absence yields `ff`.
    /// `tt` is intentionally unavailable until an explicit MUST relation and
    /// its concretization theorem are implemented in a later schema version.
    pub fn allocation_label_hold(&self, node_id: &str, allocation: &str, p: LabelPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let mut acc = Truth::False;
        for label in &node.allocation_labels {
            if label.allocation != allocation {
                continue;
            }
            let matches = match p {
                LabelPredicate::Alloc => label.predicate == EventKind::Alloc,
                LabelPredicate::Drop => label.predicate == EventKind::Drop,
                LabelPredicate::Read => label.predicate == EventKind::Read,
                LabelPredicate::Write => label.predicate == EventKind::Write,
                LabelPredicate::Use => matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write),
                LabelPredicate::AllocatorMismatch => {
                    if label.predicate != EventKind::Drop {
                        false
                    } else {
                        let allocator = self.allocations
                            .get(allocation)
                            .and_then(|a| a.allocator_contract.as_ref())
                            .map(|c| c.family.as_str())
                            .unwrap_or("unknown");
                        let deallocator = label.deallocator_contract.as_ref()
                            .map(|c| c.family.as_str())
                            .unwrap_or("unknown");
                        allocator == "unknown" || deallocator == "unknown" || allocator != deallocator
                    }
                },
            };
            if !matches {
                continue;
            }
            let value = match label.certainty {
                AllocationEventCertainty::MayAbstract => Truth::Unknown,
            };
            acc = acc.join(value);
        }
        acc
    }
}

fn validate_allocation_memory(
    node_id: &str,
    memory: &AbstractAllocationMemoryAnnotation,
    allocations: &BTreeMap<String, AbstractAllocation>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for cell in &memory.cells {
        if !allocations.contains_key(&cell.allocation) {
            return Err(format!(
                "node '{}' allocation_post references undeclared allocation '{}'",
                node_id, cell.allocation
            ));
        }
        if !seen.insert(cell.allocation.clone()) {
            return Err(format!(
                "node '{}' allocation_post contains duplicate allocation '{}'",
                node_id, cell.allocation
            ));
        }
    }
    Ok(())
}

fn validate_contract(contract: &AllocationContract, field: &str) -> Result<(), String> {
    if !matches!(contract.family.as_str(), "rust_global" | "c_malloc" | "unknown") {
        return Err(format!("{field} has unsupported family '{}'", contract.family));
    }
    if contract.operation.is_empty() {
        return Err(format!("{field} has empty operation"));
    }
    if !matches!(contract.language.as_str(), "rust" | "c" | "unknown") {
        return Err(format!("{field} has unsupported language '{}'", contract.language));
    }
    Ok(())
}


/// allocation_contracts_v2 closed producer-evidence basis vocabulary.
///
/// The producer is solely responsible for proving the basis.  The checker only
/// validates that a claimed basis is compatible with the serialized family and
/// operation; it never parses owner/allocator DefPath strings to infer a family.
fn validate_v2_deallocator_contract(contract: &AllocationContract, field: &str) -> Result<(), String> {
    let basis = contract.basis.as_deref().ok_or_else(|| {
        format!("{field} requires proof field 'basis' under allocation_contracts_v2")
    })?;

    match basis {
        "rust_box_global_drop" | "rust_vec_global_drop" => {
            if contract.family != "rust_global" || contract.operation != "drop" || contract.language != "rust" {
                return Err(format!(
                    "{field} basis '{basis}' requires family=rust_global operation=drop language=rust"
                ));
            }
            if contract.owner_def_path.as_deref().map_or(true, str::is_empty)
                || contract.allocator_def_path.as_deref().map_or(true, str::is_empty)
            {
                return Err(format!(
                    "{field} basis '{basis}' requires diagnostic owner_def_path and allocator_def_path"
                ));
            }
            if contract.callee_def_path.is_some() {
                return Err(format!(
                    "{field} basis '{basis}' does not accept explicit-call provenance"
                ));
            }
        }
        "rust_global_dealloc_api" => {
            if contract.family != "rust_global" || contract.operation != "dealloc" || contract.language != "rust" {
                return Err(format!(
                    "{field} basis '{basis}' requires family=rust_global operation=dealloc language=rust"
                ));
            }
            if contract.owner_def_path.is_some() || contract.allocator_def_path.is_some() {
                return Err(format!(
                    "{field} basis '{basis}' does not accept typed-drop provenance fields"
                ));
            }
            if contract.callee_def_path.as_deref().map_or(true, str::is_empty) {
                return Err(format!(
                    "{field} basis '{basis}' requires producer audit field callee_def_path"
                ));
            }
        }
        "structural_c_free_v1" => {
            if contract.family != "c_malloc" || contract.operation != "free" || contract.language != "c" {
                return Err(format!(
                    "{field} basis '{basis}' requires family=c_malloc operation=free language=c"
                ));
            }
            if contract.owner_def_path.is_some() || contract.allocator_def_path.is_some() || contract.callee_def_path.is_some() {
                return Err(format!(
                    "{field} basis '{basis}' does not accept Rust provenance fields"
                ));
            }
        }
        "unresolved" => {
            if contract.family != "unknown" {
                return Err(format!(
                    "{field} basis 'unresolved' must remain family=unknown"
                ));
            }
            if contract.owner_def_path.is_some() || contract.allocator_def_path.is_some() || contract.callee_def_path.is_some() {
                return Err(format!(
                    "{field} unresolved contracts must not carry provenance fields"
                ));
            }
        }
        other => return Err(format!("{field} has unsupported allocation_contracts_v2 basis '{other}'")),
    }
    Ok(())
}

fn validate_memory(
    node_id: &str,
    which: &str,
    mem: &AbstractMemoryAnnotation,
    variables: &BTreeMap<String, ProgramVariable>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for cell in &mem.cells {
        if cell.aliases.is_empty() {
            return Err(format!("node '{node_id}' {which} contains an empty alias component"));
        }
        for v in &cell.aliases {
            if !variables.contains_key(v) {
                return Err(format!("node '{node_id}' {which} references undeclared variable '{v}'"));
            }
            if !seen.insert(v.clone()) {
                return Err(format!("node '{node_id}' {which}: variable '{v}' appears in more than one abstract allocation"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(aliases: &[&str], value: CellValue) -> AbstractMemoryAnnotation {
        AbstractMemoryAnnotation { cells: vec![AbstractCell {
            aliases: aliases.iter().map(|x| x.to_string()).collect(), value
        }] }
    }

    fn base() -> AnnotatedIcfg {
        AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![
                ProgramVariable { id: "rust::x".into(), language: ProgramLanguage::Rust, display: None, function: None },
                ProgramVariable { id: "c::p".into(), language: ProgramLanguage::C, display: None, function: None },
            ],
            allocations: vec![],
            nodes: vec![AnnotatedNode {
                id: "b0".into(), successors: vec![],
                labels: vec![EventLabel { predicate: EventKind::Drop, variable: "c::p".into() }],
                semantic_labels: vec![],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: None,
                event_identity: None,
                    allocation_post: None,
                pre: mem(&["rust::x", "c::p"], CellValue::Alloc),
                post: mem(&["rust::x", "c::p"], CellValue::Top),
            }],
        }
    }

    #[test]
    fn cross_language_alias_lifts_labels_and_post_state() {
        let k = Kripke::from_annotated_icfg(base()).unwrap();
        assert_eq!(k.label_hold("b0", "rust::x", LabelPredicate::Drop), Truth::True);
        assert_eq!(k.may_hold("b0", "rust::x", MayPredicate::Alloc), Truth::Unknown);
        assert_eq!(k.may_hold("b0", "rust::x", MayPredicate::Drop), Truth::Unknown);
    }

    #[test]
    fn rejects_overlapping_abstract_components_at_one_program_point() {
        let mut input = base();
        input.nodes[0].post.cells.push(AbstractCell { aliases: vec!["rust::x".into()], value: CellValue::Freed });
        assert!(Kripke::from_annotated_icfg(input).is_err());
    }

    #[test]
    fn schema_v2_rejects_reserved_exact_abstract_certainty() {
        let raw = r#"{"predicate":"drop","allocation":"A","certainty":"exact_abstract"}"#;
        assert!(serde_json::from_str::<AllocationEventLabel>(raw).is_err());
    }


    #[test]
    fn v6s_allocation_disposition_accepts_certified_raw_pointer_drop_noop() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_disposition_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: None,
        }];
        input.nodes[0].allocation_disposition = vec![AllocationDispositionRecord {
            allocation: "A".into(),
            kind: AllocationDispositionKind::RawPointerDropNoop,
            certainty: AllocationEventCertainty::MayAbstract,
            obligation_effect: AllocationObligationEffect::NoPointeeLifecycleEffect,
            basis: "rustc_mem_drop_raw_pointer_v1".into(),
            source_variable: Some("rust::x".into()),
            target_variable: None,
            callee_def_path: Some("core::mem::drop".into()),
        }];
        Kripke::from_annotated_icfg(input).unwrap();
    }

    #[test]
    fn v6s_allocation_disposition_rejects_wrong_raw_pointer_drop_effect() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_disposition_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        input.nodes[0].allocation_disposition = vec![AllocationDispositionRecord {
            allocation: "A".into(),
            kind: AllocationDispositionKind::RawPointerDropNoop,
            certainty: AllocationEventCertainty::MayAbstract,
            obligation_effect: AllocationObligationEffect::MayDischarge,
            basis: "rustc_mem_drop_raw_pointer_v1".into(),
            source_variable: Some("rust::x".into()),
            target_variable: None,
            callee_def_path: Some("core::mem::drop".into()),
        }];
        assert!(Kripke::from_annotated_icfg(input).is_err());
    }

    #[test]
    fn entry_projection_is_deterministic_and_reachable_only() {
        let mut input = base();
        input.entry = "rust::main::bb0".into();
        input.nodes = vec![
            AnnotatedNode {
                id: "rust::main::bb0".into(),
                successors: vec!["rust::main::bb1".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "rust::main::bb1".into(),
                successors: vec![],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "rust::dead::bb0".into(),
                successors: vec![],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
        ];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", false).unwrap();
        assert_eq!(projected.entry, "rust::main::bb0");
        assert_eq!(projected.nodes.keys().cloned().collect::<Vec<_>>(), vec![
            "rust::main::bb0".to_string(),
            "rust::main::bb1".to_string(),
        ]);
    }

    #[test]
    fn intra_projection_stops_at_interprocedural_boundary() {
        let mut input = base();
        input.entry = "rust::main::bb0".into();
        input.nodes = vec![
            AnnotatedNode {
                id: "rust::main::bb0".into(),
                successors: vec!["dummyCall::rust::main::bb0".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "dummyCall::rust::main::bb0".into(),
                successors: vec!["rust::callee::bb0".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "rust::callee::bb0".into(),
                successors: vec![],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
        ];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", true).unwrap();
        assert_eq!(projected.nodes.len(), 1);
        assert!(projected.nodes.contains_key("rust::main::bb0"));
        assert!(projected.nodes["rust::main::bb0"].successors.is_empty());
    }

    #[test]
    fn schema_v2_allocation_label_uses_three_valued_abstract_certainty() {
        let mut input = base();
        input.schema_version = 2;
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: None,
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop,
            allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: None,
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::Drop), Truth::Unknown);
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::Read), Truth::False);
    }


    #[test]
    fn entry_projection_prunes_unreachable_nodes_and_quantifier_domains() {
        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec![],
            entry: "rust::main::bb0".into(),
            variables: vec![
                ProgramVariable { id: "rust::main::Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: Some("main".into()) },
                ProgramVariable { id: "rust::dead::Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: Some("dead".into()) },
            ],
            allocations: vec![
                AbstractAllocation { id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None },
                AbstractAllocation { id: "DEAD".into(), display: None, site: None, context: vec![], allocator_contract: None },
            ],
            nodes: vec![
                AnnotatedNode {
                    id: "rust::main::bb0".into(), successors: vec!["rust::main::bb1".into()],
                    labels: vec![EventLabel { predicate: EventKind::Read, variable: "rust::main::Local(_1)".into() }],
                    semantic_labels: vec![],
                    allocation_labels: vec![AllocationEventLabel { predicate: EventKind::Alloc, allocation: "A".into(), certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None }],
                    allocation_disposition: vec![],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
                AnnotatedNode {
                    id: "rust::main::bb1".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![],
                    allocation_disposition: vec![],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
                AnnotatedNode {
                    id: "rust::dead::bb0".into(), successors: vec![],
                    labels: vec![EventLabel { predicate: EventKind::Read, variable: "rust::dead::Local(_1)".into() }],
                    semantic_labels: vec![],
                    allocation_labels: vec![AllocationEventLabel { predicate: EventKind::Alloc, allocation: "DEAD".into(), certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None }],
                    allocation_disposition: vec![],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", false).unwrap();
        assert_eq!(projected.entry, "rust::main::bb0");
        assert_eq!(projected.nodes.len(), 2);
        assert!(projected.nodes.contains_key("rust::main::bb1"));
        assert!(!projected.nodes.contains_key("rust::dead::bb0"));
        assert!(projected.variables.contains_key("rust::main::Local(_1)"));
        assert!(!projected.variables.contains_key("rust::dead::Local(_1)"));
        assert!(projected.allocations.contains_key("A"));
        assert!(!projected.allocations.contains_key("DEAD"));
    }

    #[test]
    fn intra_projection_preserves_same_function_unwind_terminal() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "rust::main::bb0".into(),
            variables: vec![ProgramVariable { id: "rust::x".into(), language: ProgramLanguage::Rust, display: None, function: Some("main".into()) }],
            allocations: vec![],
            nodes: vec![
                AnnotatedNode { id: "rust::main::bb0".into(), successors: vec!["rust::main::terminate".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "rust::main::terminate".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", true).unwrap();
        assert!(projected.nodes.contains_key("rust::main::bb0"));
        assert!(projected.nodes.contains_key("rust::main::terminate"));
        assert_eq!(projected.nodes["rust::main::bb0"].successors, vec!["rust::main::terminate".to_string()]);
        assert!(projected.nodes["rust::main::terminate"].successors.is_empty());
    }

    #[test]
    fn intra_projection_is_same_function_induced_subgraph_and_stops_at_call_boundary() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "rust::main::bb0".into(),
            variables: vec![ProgramVariable { id: "v".into(), language: ProgramLanguage::Rust, display: None, function: None }],
            allocations: vec![],
            nodes: vec![
                AnnotatedNode { id: "rust::main::bb0".into(), successors: vec!["dummyCall::x".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "dummyCall::x".into(), successors: vec!["rust::callee::bb0".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "rust::callee::bb0".into(), successors: vec!["rust::callee::bb1".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "rust::callee::bb1".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let caller = k.project_from_entry("main", true).unwrap();
        assert_eq!(caller.nodes.keys().cloned().collect::<Vec<_>>(), vec!["rust::main::bb0".to_string()]);
        assert!(caller.nodes["rust::main::bb0"].successors.is_empty());

        let callee = k.project_from_entry("callee", true).unwrap();
        assert_eq!(callee.nodes.len(), 2);
        assert!(callee.nodes.contains_key("rust::callee::bb1"));
    }

    #[test]
    fn allocation_contract_capability_mismatch_label_is_unknown_and_distinct_from_drop() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_contracts_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![],
            allocator_contract: Some(AllocationContract { family: "rust_global".into(), operation: "box_allocation".into(), language: "rust".into(), basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None }),
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop, allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: Some(AllocationContract { family: "c_malloc".into(), operation: "free".into(), language: "c".into(), basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None }),
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::AllocatorMismatch), Truth::Unknown);
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::Drop), Truth::Unknown);
    }


    fn v2_contract_test_input(deallocator: AllocationContract) -> AnnotatedIcfg {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "allocation_contracts_v1".into(),
            "allocation_contracts_v2".into(),
        ];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: Some(AllocationContract {
                family: "rust_global".into(),
                operation: "box_allocation".into(),
                language: "rust".into(),
                basis: None,
                owner_def_path: None,
                allocator_def_path: None,
                callee_def_path: None,
            }),
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop,
            allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: Some(deallocator),
        }];
        input
    }

    #[test]
    fn allocation_contracts_v2_requires_v1_artifact_capability() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_contracts_v2".into()];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("requires both artifact capabilities"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_typed_drop_accepts_provenance_without_interpreting_paths() {
        let contract = AllocationContract {
            family: "rust_global".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_box_global_drop".into()),
            // Deliberately opaque diagnostic strings. Acceptance proves that
            // the checker validates the proof basis tuple and does not infer
            // family by parsing these strings.
            owner_def_path: Some("opaque::owner::diagnostic".into()),
            allocator_def_path: Some("opaque::allocator::diagnostic".into()),
            callee_def_path: None,
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap();
    }

    #[test]
    fn allocation_contracts_v2_rejects_basis_family_mismatch() {
        let contract = AllocationContract {
            family: "c_malloc".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_vec_global_drop".into()),
            owner_def_path: Some("opaque::owner".into()),
            allocator_def_path: Some("opaque::allocator".into()),
            callee_def_path: None,
        };
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires family=rust_global"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_global_dealloc_requires_producer_callee_provenance() {
        let mut contract = AllocationContract {
            family: "rust_global".into(),
            operation: "dealloc".into(),
            language: "rust".into(),
            basis: Some("rust_global_dealloc_api".into()),
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: Some("alloc::alloc::dealloc".into()),
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract.clone())).unwrap();
        contract.callee_def_path = None;
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires producer audit field callee_def_path"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_rejects_missing_basis() {
        let contract = AllocationContract {
            family: "unknown".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        };
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires proof field 'basis'"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_accepts_fail_closed_unresolved_drop() {
        let contract = AllocationContract {
            family: "unknown".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("unresolved".into()),
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap();
    }

}
