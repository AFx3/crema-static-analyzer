use std::cmp::Ordering;                     // for comparing abstract states
use std::collections::{BTreeSet, HashMap};  // store sets of variable names (for aliasing) and map allocations to cell states
use std::fmt::{self};
use log::debug;
use crate::utils::load_ffi_functions; // load_ffi_functions in utils.rs
use crate::memory_events;
use crate::mir_semantics::{mir_semantics_v2_enabled, rvalue_category};
use crate::panic_unwind::{edge_flow_kind, panic_unwind_lifecycle_v1_enabled, EdgeFlowKind};
use std::collections::HashSet;
use crate::structs::GlobalICFGNode;
use crate::structs::{MirStatement, MirTerminator, MirBasicBlock, RustAllocationDispositionEvidenceKind};
use crate::structs::LlvmJsonNode;
use crate::structs::SvfStatement;
use std::collections::VecDeque;
use once_cell::sync::Lazy;
use regex::Regex;
use crate::structs::{GlobalICFGOrdered,DummyNode,RustCallMetadata,IcfgEdge};
use std::cell::RefCell;
pub type MultiSet = HashMap<Name, usize>;
use std::collections::BTreeMap;


// a type alias  so that everywhere in the code a variable name is simply a String 
// use String as identifier for variable names (could be mir || llvm ir)
pub type Name = String;

// ################################################ ALLOCATIONS, LATTICE, JOIN, LEQ #####################################
//============================================================================
// Allocation: a set of variables that share a memory allocation.
//============================================================================
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Allocation {
    pub set: BTreeSet<Name>,
}

// grouping variables that share memory, an allocation represents a set of variables that share the same memory cell
// use a BTreeSet (which is an ordered set) so that allocation is comparable and hashable.
// This set contains all the variable names (of type Name, i.e. String) that are “aliases” of the same memory allocation
impl Allocation {
    // creates a new allocation containing a single variable
    pub fn new(var: Name) -> Self {
        let mut res = BTreeSet::new();
        res.insert(var.clone());
        Self { set: res }
    }
// inserts another variable into this allocation. If find that another variable aliases the same memory, can insert it into the allocation’s set
    pub fn insert(&mut self, var: Name) {
        self.set.insert(var);
    }
}
// print out the allocation for debugging ( print the set of variable names)
impl fmt::Debug for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.set)
    }
}
//============================================================================
// CellValue lattice.
//
// Theoretical order:
//   BOTTOM <= every value
//   ALLOC  <= MB, IMMB, MV
//   BOXTIMES, FREED, MB, IMMB, MV are pairwise incomparable unless related
//   by the previous ALLOC edges
//   every value <= TOP
//
// BOXTIMES abstracts scalar / non-heap local values (and the corresponding
// residual concrete layouts in the formal model).  In particular it is NOT
// an alias/heap-state marker and is incomparable with ALLOC/FREED/MB/IMMB/MV.
//============================================================================
//
//                         TOP
//             /       /    |    \       \
//            MB     IMMB   MV   FREED   BOXTIMES
//             \       |    /
//                    ALLOC
//                      |
////                   BOTTOM
//============================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum CellValue {
    /// No normally represented concrete state (least element).
    BOTTOM,
    /// Scalar / non-heap abstract value from the formal model (\boxtimes).
    BOXTIMES,
    /// Owned allocated heap cell.
    ALLOC,
    /// Heap cell known to have been freed.
    FREED,
    /// Mutable borrow.
    MB,
    /// Immutable borrow.
    IMMB,
    /// Ownership forgotten / raw.
    MV,
    /// Completely imprecise abstract value (greatest element).
    TOP,
}

impl CellValue {
    /// Partial order of the CellValue lattice.
    pub fn leq(self, other: Self) -> bool {
        use CellValue::*;

        match (self, other) {
            (x, y) if x == y => true,
            (BOTTOM, _) => true,
            (_, TOP) => true,
            (ALLOC, MB | IMMB | MV) => true,
            _ => false,
        }
    }

    /// Least upper bound.
    ///
    /// Deriving join from `leq` keeps the implementation synchronized with
    /// the Hasse diagram: comparable values join to the larger one; all
    /// remaining incomparable pairs join to TOP.
    pub fn join(self, other: Self) -> Self {
        if self.leq(other) {
            other
        } else if other.leq(self) {
            self
        } else {
            CellValue::TOP
        }
    }

    /// Greatest lower bound.
    ///
    /// The only non-trivial incomparable pairs with a common lower bound
    /// strictly above BOTTOM are pairs among MB/IMMB/MV, whose GLB is ALLOC.
    pub fn meet(self, other: Self) -> Self {
        use CellValue::*;

        if self.leq(other) {
            return self;
        }
        if other.leq(self) {
            return other;
        }

        match (self, other) {
            (MB, IMMB) | (IMMB, MB)
            | (MB, MV) | (MV, MB)
            | (IMMB, MV) | (MV, IMMB) => ALLOC,
            _ => BOTTOM,
        }
    }

    pub fn is_default(self) -> bool {
        self == CellValue::BOTTOM
    }
}

// TRAIT 
impl PartialOrd for CellValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
         if *self == *other {
            Some(Ordering::Equal)
         } else if self.leq(*other) {
            Some(Ordering::Less)
         } else if other.leq(*self) {
            Some(Ordering::Greater)
         } else {
            None
         }
    }
}

// ################################################ ABSTRACT MEMORY  #####################################
//============================================================================
// AbstractMemory: indicates an abstract memory, i.e. a mapping sigma: Var -> CellValues
// Internally, we group aliased variables into an Allocation.
//============================================================================

#[derive(Clone, PartialEq, Eq)]
pub struct AbstractMemory {
    pub state: BTreeMap<Allocation, CellValue>,  
}

impl Default for AbstractMemory {
    fn default() -> Self {
        Self { state: BTreeMap::new() }          
    }
}

impl fmt::Debug for AbstractMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.state)
    }
}


/// Preserve the analyzer's existing convention that a projected name also
/// records its prefixes in the same allocation.
fn insert_name_and_prefixes(alloc: &mut Allocation, var: &Name) {
    let parts: Vec<&str> = var.split('.').collect();
    let mut prefix = String::new();

    for part in parts {
        if prefix.is_empty() {
            prefix.push_str(part);
        } else {
            prefix.push('.');
            prefix.push_str(part);
        }
        alloc.insert(prefix.clone());
    }
}

impl AbstractMemory {
    /// All locals explicitly represented in this sparse abstract memory.
    fn all_vars(&self) -> BTreeSet<Name> {
        self.state
            .keys()
            .flat_map(|alloc| alloc.set.iter().cloned())
            .collect()
    }

    /// Canonical may-alias pairs represented by this memory.
    ///
    /// The implementation stores alias information as equivalence classes.
    /// We therefore expose all unordered pairs in each class.  This is used
    /// only to order/join the additional implementation-level alias component;
    /// it is deliberately separate from the formal CellValue lattice.
    fn alias_pairs(&self) -> BTreeSet<(Name, Name)> {
        let mut pairs = BTreeSet::new();

        for alloc in self.state.keys() {
            let members: Vec<_> = alloc.set.iter().cloned().collect();
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    pairs.insert((members[i].clone(), members[j].clone()));
                }
            }
        }

        pairs
    }

    /// Pointwise CellValue order plus inclusion of implementation-level
    /// may-alias information.
    ///
    /// More may-alias pairs means less precision, hence alias-set inclusion
    /// follows the same direction as the abstract order.
    pub fn leq(&self, other: &Self) -> bool {
        let self_vars = self.all_vars();
        let other_vars = other.all_vars();
        let all_vars: BTreeSet<Name> =
            self_vars.union(&other_vars).cloned().collect();

        let values_leq = all_vars
            .iter()
            .all(|var| self.get_cell_value(var).leq(other.get_cell_value(var)));

        values_leq && self.alias_pairs().is_subset(&other.alias_pairs())
    }

    /// Least upper bound of two implementation memories.
    ///
    /// Cell values are joined pointwise. Alias information is joined as the
    /// equivalence closure of the union of the two may-alias relations. Since
    /// one Allocation carries one CellValue, every resulting alias component
    /// is assigned the join of the pointwise values of all of its members.
    ///
    /// This is conservative: if two paths connect a chain a~b and b~c, the
    /// implementation represents the merged may-alias component {a,b,c}.
    pub fn union(&self, other: &Self) -> Self {
        let self_vars = self.all_vars();
        let other_vars = other.all_vars();
        let all_vars: BTreeSet<Name> =
            self_vars.union(&other_vars).cloned().collect();

        if all_vars.is_empty() {
            return AbstractMemory::default();
        }

        // Build the undirected graph induced by alias components from both
        // incoming memories. Connecting each member to a representative is
        // sufficient; connected components compute the equivalence closure.
        let mut adjacency: BTreeMap<Name, BTreeSet<Name>> = all_vars
            .iter()
            .cloned()
            .map(|v| (v, BTreeSet::new()))
            .collect();

        for mem in [self, other] {
            for alloc in mem.state.keys() {
                let mut members = alloc.set.iter();
                if let Some(first) = members.next() {
                    for member in members {
                        adjacency
                            .entry(first.clone())
                            .or_default()
                            .insert(member.clone());
                        adjacency
                            .entry(member.clone())
                            .or_default()
                            .insert(first.clone());
                    }
                }
            }
        }

        let mut result = AbstractMemory::default();
        let mut visited = BTreeSet::new();

        for start in all_vars {
            if visited.contains(&start) {
                continue;
            }

            let mut component = BTreeSet::new();
            let mut queue = VecDeque::new();
            queue.push_back(start.clone());
            visited.insert(start);

            while let Some(v) = queue.pop_front() {
                component.insert(v.clone());
                if let Some(neighbours) = adjacency.get(&v) {
                    for n in neighbours {
                        if visited.insert(n.clone()) {
                            queue.push_back(n.clone());
                        }
                    }
                }
            }

            // Pointwise value join for each local, followed by a join across
            // the alias component so that one canonical Allocation has one
            // sound CellValue.
            let mut component_value = CellValue::BOTTOM;
            for var in &component {
                let pointwise = self
                    .get_cell_value(var)
                    .join(other.get_cell_value(var));
                component_value = component_value.join(pointwise);
            }

            if component_value != CellValue::BOTTOM {
                result
                    .state
                    .insert(Allocation { set: component }, component_value);
            }
        }

        result
    }

    /// Set the state of the allocation containing `var`.
    ///
    /// IMPORTANT: equal CellValues do not imply aliasing.  If `var` is not
    /// currently tracked, a fresh singleton allocation is created.  If `var`
    /// already belongs to an alias component, changing a non-BOTTOM heap state
    /// updates that whole component, as aliases denote the same allocation.
    ///
    /// Setting a local to BOTTOM removes that local from the component while
    /// preserving the previous value for the remaining aliases.
    pub fn set_cell_value(&mut self, var: &Name, cell_value: CellValue) {
        if let Some(mut alloc) = self.get_allocation(var) {
            let previous_value = self
                .state
                .remove(&alloc)
                .expect("allocation returned by get_allocation must exist");

            if cell_value == CellValue::BOTTOM {
                alloc.set.remove(var);
                if !alloc.set.is_empty() {
                    self.state.insert(alloc, previous_value);
                }
                return;
            }

            // Preserve the existing projection-prefix convention used by the
            // analyzer, but do not merge unrelated allocations by CellValue.
            insert_name_and_prefixes(&mut alloc, var);
            self.state.insert(alloc, cell_value);
            return;
        }

        if cell_value != CellValue::BOTTOM {
            let mut alloc = Allocation::new(var.clone());
            insert_name_and_prefixes(&mut alloc, var);
            self.state.insert(alloc, cell_value);
        }
    }

    /// Explicit local overwrite: detach `var` from any previous alias
    /// component and create a fresh singleton representation for the new
    /// local value. This is intentionally distinct from heap-state updates.
    pub fn assign_local_value(&mut self, var: &Name, cell_value: CellValue) {
        if let Some(mut old_alloc) = self.get_allocation(var) {
            let previous_value = self
                .state
                .remove(&old_alloc)
                .expect("allocation returned by get_allocation must exist");

            old_alloc.set.remove(var);
            if !old_alloc.set.is_empty() {
                self.state.insert(old_alloc, previous_value);
            }
        }

        if cell_value != CellValue::BOTTOM {
            let mut fresh = Allocation::new(var.clone());
            insert_name_and_prefixes(&mut fresh, var);
            self.state.insert(fresh, cell_value);
        }
    }

    pub fn get_cell_value(&self, var: &Name) -> CellValue {
        for (alloc, &cell_value) in &self.state {
            if alloc.set.contains(var) {
                return cell_value;
            }
        }
        CellValue::BOTTOM
    }

    pub fn get_allocation(&self, var: &Name) -> Option<Allocation> {
        for alloc in self.state.keys() {
            if alloc.set.contains(var) {
                return Some(alloc.clone());
            }
        }
        None
    }

    /// Propagate an allocation identity from `from` to `to`.
    ///
    /// This operation is used only when the MIR/ICFG semantics provide
    /// evidence that the two locals denote the same allocation. It never
    /// infers aliasing merely from equal abstract CellValues.
    pub fn propagate_cell_value(&mut self, from: &Name, to: &Name) {
        debug!("Propagate cell value from {} to {}", from, to);
        debug!("Current state: {:?}", self.state);

        if from == to {
            return;
        }

        let from_value = self.get_cell_value(from);

        if from_value == CellValue::BOTTOM {
            // No represented source allocation: an assignment/copy from
            // BOTTOM cannot justify an alias relation. Detach only `to`.
            self.assign_local_value(to, CellValue::BOTTOM);
            debug!("After propagation, state: {:?}", self.state);
            return;
        }

        let from_alloc = self
            .get_allocation(from)
            .expect("non-BOTTOM source must belong to an allocation");

        // Already aliases: nothing to change.
        if self
            .get_allocation(to)
            .as_ref()
            .is_some_and(|to_alloc| *to_alloc == from_alloc)
        {
            return;
        }

        // Overwriting `to` must detach only `to` from its previous component.
        // Its old aliases continue to denote the old allocation.
        if let Some(mut old_to_alloc) = self.get_allocation(to) {
            let old_to_value = self
                .state
                .remove(&old_to_alloc)
                .expect("allocation returned by get_allocation must exist");

            old_to_alloc.set.remove(to);
            if !old_to_alloc.set.is_empty() {
                self.state.insert(old_to_alloc, old_to_value);
            }
        }

        // Attach `to` to the source allocation.
        let mut source_alloc = self
            .get_allocation(from)
            .expect("source allocation must still exist after detaching destination");
        self.state.remove(&source_alloc);
        source_alloc.insert(to.clone());
        self.state.insert(source_alloc, from_value);

        debug!("After propagation, state: {:?}", self.state);
    }
}

impl PartialOrd for AbstractMemory {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_leq_other = self.leq(other);
        let other_leq_self = other.leq(self);

        match (self_leq_other, other_leq_self) {
            (true, true) => Some(Ordering::Equal),
            (true, false) => Some(Ordering::Less),
            (false, true) => Some(Ordering::Greater),
            (false, false) => None,
        }
    }
}

//============================================================================
// AbstractState: represents an abstract state, i.e. a mapping from basic blocks to abstract memories.
// B -> A_mem
//============================================================================
#[derive(Clone, PartialEq)]
pub struct AbstractState {
    state_map: HashMap<String, AbstractMemory>,
}

impl Default for AbstractState {
    fn default() -> Self {
        Self {
            state_map: HashMap::new(),
        }
    }
}

impl fmt::Debug for AbstractState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.state_map)
    }
}

impl PartialOrd for AbstractState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_leq_other = self.leq(other);
        let other_leq_self = other.leq(self);

        match (self_leq_other, other_leq_self) {
            (true, true) => Some(Ordering::Equal),
            (true, false) => Some(Ordering::Less),
            (false, true) => Some(Ordering::Greater),
            (false, false) => None,
        }
    }
}

impl AbstractState {
    /// Pointwise order over basic blocks, using the implementation-level
    /// AbstractMemory order at each block.
    pub fn leq(&self, other: &Self) -> bool {
        let all_keys: BTreeSet<&String> = self
            .state_map
            .keys()
            .chain(other.state_map.keys())
            .collect();

        all_keys.into_iter().all(|key| {
            let self_mem = self.state_map.get(key).cloned().unwrap_or_default();
            let other_mem = other.state_map.get(key).cloned().unwrap_or_default();
            self_mem.leq(&other_mem)
        })
    }

    // retrieves the abstract memory for a given basic block
    pub fn get(&self, block: &String) -> Option<AbstractMemory> {
        self.state_map.get(block).cloned()
    }
    // inserts (or updates) the abstract memory for a basic block
    pub fn insert(&mut self, block: String, mem: AbstractMemory) {
        self.state_map.insert(block, mem);
    }

    pub fn get_allocation(&self, var: &Name) -> Option<Allocation> {
        // for each allocation in global abstract state, check if it contains var
        for alloc in self.state_map.values() {
            for (alloc_obj, _cell_val) in alloc.state.iter() {
                if alloc_obj.set.contains(var) {
                    return Some(alloc_obj.clone());
                }
            }
        }
        None
    }

    /// Union of every alias component containing `var` across all program
    /// points represented in this AbstractState.
    ///
    /// `state_map` is a HashMap, so selecting the first matching allocation is
    /// not a stable way to reconstruct a global MAY-alias relation.  Different
    /// basic blocks may legitimately contain different alias components for the
    /// same local (for example `{p,q}` on one path and `{p,q,r}` on another).
    /// The detector is path-insensitive, therefore its global free-flow closure
    /// must conservatively include every alias witnessed at any block.
    fn get_may_aliases(&self, var: &Name) -> BTreeSet<Name> {
        let mut aliases = BTreeSet::new();

        for mem in self.state_map.values() {
            for alloc in mem.state.keys() {
                if alloc.set.contains(var) {
                    aliases.extend(alloc.set.iter().cloned());
                }
            }
        }

        aliases
    }
}

// ============================================================================
// TAINT STATE
// ============================================================================
// define a TaintState as a mapping from block identifier to a mapping from variable names
// to a set of taint markers (e.g., "assign", "free", "use").
// ----------------------------------------------------------------------
// TAINT STATE TYPES
// ----------------------------------------------------------------------
pub type Taint = HashSet<String>;           // e.g. "assign", "free", "use"
pub type TaintStateMap = HashMap<Name, Taint>; // mapping: variable -> markers (for one basic block)
pub type TaintState = HashMap<String, TaintStateMap>; // mapping: block ID -> (var -> markers)

/// Positive allocator-family provenance for a pointer returned by the C
/// malloc/calloc family.
///
/// This is an auxiliary MAY property rather than a CellValue: malloc/calloc
/// may return null, so the primary lattice cannot soundly claim definite
/// ALLOC.
const TAINT_C_MALLOC_FAMILY: &str = "alloc_family:c_malloc";
const TAINT_ASSIGN: &str = "assign";

fn taint_has_c_malloc_origin(tags: &Taint) -> bool {
    tags.contains(TAINT_C_MALLOC_FAMILY)
}


/// Least upper bound for the auxiliary may-taint/provenance component.
///
/// Taint is independent from the CellValue lattice.  In particular, when a
/// tracked raw allocation (MV) joins a scalar/non-heap value (BOXTIMES), the
/// CellValue becomes TOP, but the path on which the allocation exists must not
/// be forgotten.  Therefore markers are joined by set union, not reconstructed
/// solely from the joined CellValue.
///
/// The final loop adds the two markers that are semantically implied by a
/// precise joined memory value.  It never removes markers already justified by
/// one incoming path.
fn join_taint_maps(
    old_taint: &TaintStateMap,
    new_taint: &TaintStateMap,
    joined_mem: &AbstractMemory,
) -> TaintStateMap {
    let mut joined = old_taint.clone();

    for (var, tags) in new_taint {
        joined
            .entry(var.clone())
            .or_default()
            .extend(tags.iter().cloned());
    }

    for (alloc, &value) in &joined_mem.state {
        let implied = match value {
            CellValue::MV => Some("assign"),
            CellValue::FREED => Some("free"),
            _ => None,
        };

        if let Some(tag) = implied {
            for var in &alloc.set {
                joined
                    .entry(var.clone())
                    .or_default()
                    .insert(tag.to_string());
            }
        }
    }

    joined
}
// method for taint state that takes as input a basic block and returns the taint state for that block
// takes a reference to the global taint state (TaintState) and a basic block (GlobalICFGNode)

//used for testing
pub fn get_taint_state_for_block(taint_state: &TaintState, block: &GlobalICFGNode) -> TaintStateMap {
    // build the key according to the node type:
    let key = match block {
        GlobalICFGNode::Mir(bb) => {
            // MIR node: e.g., "rust::main::bb4"
            format!("rust::main::bb{}", bb.block_id)
        },
        GlobalICFGNode::Llvm(llvm_node) => {
            // LLVM nodes: e.g., "llvm::cast_and_free_pointer::node100582464396320"
            format!("llvm::{}::node{}", llvm_node.node_kind_string, llvm_node.node_id)
        },
        GlobalICFGNode::DummyCall(dummy_call) => {
            // dummy call nodes: e.g., "dummyCall::rust::main::bb2"
            //format!("dummyCall::{}", dummy_call.id)
            dummy_call.id.clone()
        },
        GlobalICFGNode::DummyRet(dummy_ret) => {
            // dummy return nodes: e.g., "dummyRet::rust::main::bb3"
            //format!("dummyRet::{}", dummy_ret.id)
            dummy_ret.id.clone()
        },
        GlobalICFGNode::Terminal(terminal) => format!("terminal::{}", terminal.reason),
    };

    taint_state.get(&key).cloned().unwrap_or_default()
}


// ----------------------------------------------------------------------
// TRANSFER FUNCTION DISPATCH
// ----------------------------------------------------------------------
pub fn transfer_function(
    node_id: &str,
    node: &GlobalICFGNode,
    in_mem: &AbstractMemory,
    in_taint: &TaintStateMap,
) -> (AbstractMemory, TaintStateMap) {
    match node {
        GlobalICFGNode::Mir(bb) => process_mir_basic_block(bb, in_mem, in_taint),
        GlobalICFGNode::DummyCall(dummy_call) => {
            transfer_dummycall_node(dummy_call, in_mem, in_taint)
        }
        GlobalICFGNode::DummyRet(dummy_ret) => {
            transfer_dummyret_node(dummy_ret, in_mem, in_taint)
        }
        GlobalICFGNode::Llvm(llvm_node) => {
            transfer_llvm_node(node_id, llvm_node, in_mem, in_taint)
        }
        GlobalICFGNode::Terminal(_) => (in_mem.clone(), in_taint.clone()),
    }
}


// normalize var names di var (eg. "_5") in "Local(_5)"
fn normalize(var: &str) -> String {
    if let Some(inner) = var.strip_prefix("Local ") {
        format!("Local({})", inner.trim())
    } else {
        format!("Local({})", var.trim_start_matches('_'))
    }
}

pub fn transfer_dummyret_node(
    dummy: &DummyNode,
    in_mem: &AbstractMemory,
    in_taint: &TaintStateMap,
) -> (AbstractMemory, TaintStateMap) {
    let mut mem_ret = in_mem.clone();
    let mut taint_ret = in_taint.clone();

    if let (Some(mir_var), Some(llvm_var)) = (&dummy.mir_var, &dummy.llvm_var) {
        if dummy.is_internal.unwrap_or(false) {
            let full_mir = normalize(mir_var);
            let full_llvm = normalize(llvm_var);

            let old = mem_ret.get_cell_value(&full_mir);
            let ret = mem_ret.get_cell_value(&full_llvm);
            mem_ret.set_cell_value(&full_mir, old.join(ret));

            let mut t0 = taint_ret.remove(&full_mir).unwrap_or_default();
            let t1 = taint_ret.remove(&full_llvm).unwrap_or_default();
            t0.extend(t1);
            taint_ret.insert(full_mir, t0);
        } else {
            // C -> Rust return bridge.
            let full_mir = full_local_name(mir_var);
            let returned_tags =
                taint_ret.get(llvm_var).cloned().unwrap_or_default();

            if taint_has_c_malloc_origin(&returned_tags) {
                mem_ret.assign_local_value(&full_mir, CellValue::TOP);
                let entry = taint_ret.entry(full_mir).or_default();
                entry.extend(returned_tags);
                entry.insert(TAINT_ASSIGN.to_string());
            }
        }
    }

    (mem_ret, taint_ret)
}


// ----------------------------------------------------------------------
// HELPERS
// ----------------------------------------------------------------------
pub fn extract_moved_var(s: &str) -> String {
    if let Some(after_move) = s.split("move ").nth(1) {
        // take the first token after "move " and remove a trailing ')' if present
        let token = after_move.split_whitespace().next().unwrap_or("");
        return token.trim_end_matches(')').to_string();
    }
    "".to_string()
}
pub fn extract_copied_var(s: &str) -> String {
    if let Some(after_move) = s.split("copy ").nth(1) {
        // take the first token after "copy " and remove a trailing ')' if present
        let token = after_move.split_whitespace().next().unwrap_or("");
        return token.trim_end_matches(')').to_string();
    }
    "".to_string()
}


pub fn full_local_name(var: &str) -> String {
    // if the variable already starts with "Local(", then remove any extra suffix (like [mutable])
    if var.starts_with("Local(") {
        // if there is a space ("Local(_1) [mutable]"), take only the part until the first space
        if let Some(pos) = var.find(' ') {
            return var[..pos].to_string();
        }
        return var.to_string();
    } else {
        format!("Local({})", var)
    }
}

// ----------------------------------------------------------------------
// EXPLICIT std::mem::drop HELPERS
// ----------------------------------------------------------------------
// Keep the semantics of explicit drop calls in one place so that the
// abstract transfer function and the final memory-issue detector agree.
//
// Rust raw pointers (*mut T / *const T) do not own the pointee. Calling
// std::mem::drop on a raw pointer only consumes/copies the pointer value;
// it does NOT invoke the allocator and therefore must not free the pointee.
//
// For non-raw explicit drops, CREMA only changes heap state if the dropped
// variable already belongs to an allocation tracked by the abstract memory.
fn is_explicit_mem_drop(s: &str) -> bool {
    s.contains("std::mem::drop::<") || s.contains("core::mem::drop::<")
}

fn is_raw_pointer_mem_drop(s: &str) -> bool {
    const PREFIXES: [&str; 2] = ["std::mem::drop::<", "core::mem::drop::<"];

    for prefix in PREFIXES {
        if let Some(pos) = s.find(prefix) {
            let ty = s[pos + prefix.len()..].trim_start();
            return ty.starts_with("*mut ") || ty.starts_with("*const ");
        }
    }

    false
}

// Extract the MIR local passed to an explicit drop call.
// Owning values are normally moved; Copy values may appear as `copy`.
fn explicit_drop_arg_from_details(details: &str) -> Option<Name> {
    let moved = extract_moved_var(details);
    if !moved.is_empty() {
        return Some(full_local_name(&moved));
    }

    let copied = extract_copied_var(details);
    if !copied.is_empty() {
        return Some(full_local_name(&copied));
    }

    None
}

pub fn get_node_by_id(icfg: &GlobalICFGOrdered, id: &String) -> GlobalICFGNode {
    for (node_id, node) in &icfg.ordered_nodes {
        if node_id == id {
            return match node {
                GlobalICFGNode::DummyCall(dummy) => {
                    let mut updated = dummy.clone();
                    // set the dummy node's internal id to match the key in the ordered nodes
                    updated.id = node_id.clone();
                    GlobalICFGNode::DummyCall(updated)
                },
                _ => node.clone(),
            };
        }
    }
    // return a dummy node if not found
    GlobalICFGNode::DummyCall(DummyNode {
        dummy_node_name: "NODE_NOT_FOUND(dummy)".to_string(),
        incoming_edge: "".to_string(),
        outgoing_edge: "".to_string(),
        id: id.clone(),
        mir_var: None,
        llvm_var: None,
        argument_bindings: Vec::new(),
        is_internal: None,
    })
}
// ----------------------------------------------------------------------
// HELPER: update_state
pub fn update_state(mut mem: AbstractMemory, var: &str, val: CellValue) -> AbstractMemory {
    mem.set_cell_value(&var.to_string(), val);
    mem
}
// helper to extract the "arg" value from the details string.
fn extract_arg_from_details(s: &str) -> Option<String> {
    if let Some(start) = s.find("\"arg\":") {
        let rest = &s[start + "\"arg\":".len()..];
        if let Some(first_quote) = rest.find('\"') {
            let rest = &rest[first_quote + 1..];
            if let Some(end_quote) = rest.find('\"') {
                return Some(rest[..end_quote].to_string());
            }
        }
    }
    None
}
// ----------------------------------------------------------------------
// PHASE 3: BOXTIMES-aware MIR rvalue abstraction
// ----------------------------------------------------------------------
//
// The formal model maps scalar/non-heap values to BOXTIMES. The implementation
// receives rustc's Debug representation of MIR Rvalues, so this classifier is
// deliberately positive: BOXTIMES is produced only when the textual MIR form
// itself is sufficient to establish a scalar/unit result.
//
// Rust-specific forms outside this recognized core keep the Phase-2 fallback.
// We do NOT globally map every unhandled Rvalue to TOP in this phase; that is a
// separate precision/soundness decision documented in PHASE3_BOTTOM_TOP_AUDIT.

static SCALAR_INT_CONST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^const -?[0-9]+_(?:i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize)$"
    ).unwrap()
});

static SCALAR_FLOAT_CONST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^const -?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?_(?:f32|f64)$"
    ).unwrap()
});

static SCALAR_CHAR_CONST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^const '(?:\\.|[^\\'])+'$").unwrap()
});

static LOCAL_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"_[0-9]+").unwrap()
});

fn is_scalar_type_name(ty: &str) -> bool {
    matches!(
        ty.trim(),
        "bool"
            | "char"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "()"
    )
}

fn is_scalar_const_rvalue(e: &str) -> bool {
    let e = e.trim();

    e == "const true"
        || e == "const false"
        || e == "const ()"
        || SCALAR_INT_CONST_RE.is_match(e)
        || SCALAR_FLOAT_CONST_RE.is_match(e)
        || SCALAR_CHAR_CONST_RE.is_match(e)
}

fn is_scalar_nullary_rvalue(e: &str) -> bool {
    let e = e.trim();

    // These MIR operations return integer-like scalars independently of the
    // memory-management abstraction of their operand/type.
    e.starts_with("SizeOf(")
        || e.starts_with("AlignOf(")
        || e.starts_with("Len(")
        || e.starts_with("Discriminant(")
}

fn scalar_cast_target(e: &str) -> Option<&str> {
    // Examples:
    //   copy _29 as usize (Transmute)
    //   copy _2 as i64 (IntToInt)
    //
    // Pointer targets are intentionally excluded: they may denote tracked
    // allocations and must not become BOXTIMES merely because they are casts.
    let (_, after_as) = e.rsplit_once(" as ")?;
    let target = after_as.split_whitespace().next()?;
    is_scalar_type_name(target).then_some(target)
}

fn extract_first_local_token(e: &str) -> Option<String> {
    LOCAL_TOKEN_RE
        .find(e)
        .map(|m| m.as_str().to_string())
}

fn exact_local_operand(e: &str, keyword: &str) -> Option<String> {
    let rest = e.trim().strip_prefix(keyword)?.trim();

    // Accept only a direct `_N` operand. Casts, projections and aggregates
    // have different semantics and must not be mistaken for a direct use.
    if LOCAL_TOKEN_RE
        .find(rest)
        .is_some_and(|m| m.start() == 0 && m.end() == rest.len())
    {
        Some(rest.to_string())
    } else {
        None
    }
}

fn split_top_level_binary_args(s: &str) -> Option<(&str, &str)> {
    let mut paren = 0usize;
    let mut bracket = 0usize;
    let mut brace = 0usize;

    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            ',' if paren == 0 && bracket == 0 && brace == 0 => {
                return Some((s[..idx].trim(), s[idx + 1..].trim()));
            }
            _ => {}
        }
    }

    None
}

fn binary_rvalue_args(e: &str) -> Option<(&str, &str)> {
    const OPS: [&str; 16] = [
        "Add", "Sub", "Mul", "Div", "Rem",
        "BitXor", "BitAnd", "BitOr", "Shl", "Shr",
        "Eq", "Lt", "Le", "Ne", "Ge", "Gt",
    ];

    let e = e.trim();
    for op in OPS {
        let prefix = format!("{}(", op);
        if e.starts_with(&prefix) && e.ends_with(')') {
            let inner = &e[prefix.len()..e.len() - 1];
            return split_top_level_binary_args(inner);
        }
    }

    None
}

fn unary_rvalue_arg(e: &str) -> Option<&str> {
    const OPS: [&str; 2] = ["Neg", "Not"];

    let e = e.trim();
    for op in OPS {
        let prefix = format!("{}(", op);
        if e.starts_with(&prefix) && e.ends_with(')') {
            return Some(e[prefix.len()..e.len() - 1].trim());
        }
    }

    None
}


fn comparison_rvalue_args(e: &str) -> Option<(&str, &str)> {
    const OPS: [&str; 6] = ["Eq", "Lt", "Le", "Ne", "Ge", "Gt"];

    let e = e.trim();
    for op in OPS {
        let prefix = format!("{}(", op);
        if e.starts_with(&prefix) && e.ends_with(')') {
            let inner = &e[prefix.len()..e.len() - 1];
            return split_top_level_binary_args(inner);
        }
    }

    None
}

fn pointer_cast_source(e: &str) -> Option<String> {
    let e = e.trim();
    let target_is_raw_pointer = e
        .rsplit_once(" as ")
        .and_then(|(_, after_as)| after_as.split_whitespace().next())
        .is_some_and(|ty| ty.starts_with("*mut") || ty.starts_with("*const"));

    if !target_is_raw_pointer {
        return None;
    }

    extract_first_local_token(e)
}

fn pointer_offset_source(e: &str) -> Option<String> {
    let e = e.trim();
    if !e.starts_with("Offset(") || !e.ends_with(')') {
        return None;
    }

    let inner = &e["Offset(".len()..e.len() - 1];
    let (ptr_operand, _) = split_top_level_binary_args(inner)?;
    extract_first_local_token(ptr_operand)
}

/// Return the directly borrowed stack local for MIR Ref/AddressOf forms such as
/// `&_1`, `&mut _1`, `&raw const _1`, and `&raw mut _1`.
///
/// This is a relation between MIR stack places. It is deliberately NOT a heap
/// alias relation: a reference to a local containing `Box<T>` points to the
/// local storage of the Box value, not to the Box allocation.
fn direct_stack_borrow_source(e: &str) -> Option<Name> {
    let e = e.trim();

    if e.contains("(*") {
        return None;
    }

    let rest = if let Some(rest) = e.strip_prefix("&raw const ") {
        rest
    } else if let Some(rest) = e.strip_prefix("&raw mut ") {
        rest
    } else if let Some(rest) = e.strip_prefix("&mut ") {
        rest
    } else if let Some(rest) = e.strip_prefix('&') {
        rest.trim_start()
    } else {
        return None;
    };

    let local = LOCAL_TOKEN_RE.find(rest)?;
    if local.start() == 0 && local.end() == rest.len() {
        Some(full_local_name(local.as_str()))
    } else {
        None
    }
}

/// Extract the local containing the reference/pointer dereferenced by a direct
/// MIR value load (`copy (*_N)` / `move (*_N)`).
fn direct_deref_value_source(e: &str) -> Option<Name> {
    let e = e.trim();

    for prefix in ["copy (*", "move (*"] {
        if let Some(rest) = e.strip_prefix(prefix) {
            let local = LOCAL_TOKEN_RE.find(rest)?;
            if local.start() != 0 {
                return None;
            }

            if rest[local.end()..].trim() == ")" {
                return Some(full_local_name(local.as_str()));
            }
        }
    }

    None
}

/// Canonical detector spelling: `_4`, `Local(_4)`, and
/// `Local(_4) [mutable]` all become `Local(_4)`.
fn canonical_mir_local(name: &str) -> Name {
    full_local_name(name.trim())
}

/// Return the MIR locals participating in a raw-pointer cast assignment.
///
/// The detector records them as free-flow participants, but deliberately does
/// not union them here.  The later AbstractState alias reconciliation is the
/// semantic proof that the cast preserved an already tracked allocation. This
/// avoids manufacturing aliases for integer-to-pointer or otherwise untracked
/// casts while still making a real `raw.cast::<c_void>()` visible to direct
/// foreign `free` resolution.
fn detector_pointer_cast_participants(
    stmt: &MirStatement,
) -> Option<(Name, Name)> {
    let dest = canonical_mir_local(stmt.place.as_ref()?);
    let source = pointer_cast_source(stmt.rvalue.as_deref()?)?;
    Some((dest, canonical_mir_local(&source)))
}

/// Immediate may-targets of a stack reference.
fn resolve_stack_ref_values(
    reference: &Name,
    refs: &BTreeMap<Name, BTreeSet<Name>>,
) -> BTreeSet<Name> {
    refs.get(reference).cloned().unwrap_or_default()
}


/// MIR pretty-print spelling for a closure aggregate, e.g.
///
/// `{closure@...} { ptr: move _4, other: copy _7 }`
///
/// The operand order is the closure-field order used by later `.0`, `.1`, ...
/// projections in the closure body.
fn closure_aggregate_capture_operands(e: &str) -> Option<Vec<Name>> {
    let e = e.trim();
    if !e.starts_with("{closure@") {
        return None;
    }

    let (_, fields) = e.split_once("} {")?;
    let fields = fields.strip_suffix('}')?.trim();

    if fields.is_empty() {
        return Some(Vec::new());
    }

    let mut out = Vec::new();
    for field in fields.split(',') {
        let (_, rhs) = field.split_once(':')?;
        let rhs = rhs.trim();

        if let Some(local) = exact_local_operand(rhs, "move ")
            .or_else(|| exact_local_operand(rhs, "copy "))
        {
            out.push(full_local_name(&local));
        } else {
            // A closure capture that is not a direct MIR local operand cannot
            // yet be represented by the current detector relation.
            return None;
        }
    }

    Some(out)
}

static CLOSURE_FIELD_COPY_FOR_DEREF_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^deref_copy\s+\((?:\(\*_[0-9]+\)|_[0-9]+)\.([0-9]+):"
    ).unwrap()
});

/// Field index read by a CopyForDeref from a closure/self aggregate.
///
/// Recognized examples:
/// - `deref_copy ((*_1).0: &*mut i32)`
/// - `deref_copy (_1.0: &LockFreeStack<i32>)`
fn closure_field_copy_for_deref_index(e: &str) -> Option<usize> {
    let caps = CLOSURE_FIELD_COPY_FOR_DEREF_RE.captures(e.trim())?;
    caps.get(1)?.as_str().parse::<usize>().ok()
}

fn is_copy_for_deref_rvalue(e: &str) -> bool {
    e.trim().starts_with("deref_copy ")
}

static DIRECT_DEREF_USE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:copy|move) \(\*_[0-9]+\)$").unwrap()
});

/// Rvalue::Use of a directly dereferenced place, e.g. `copy (*_8)`.
///
/// The value exists, but the current CellValue domain does not model arbitrary
/// memory contents behind a stack/raw reference; TOP is therefore conservative.
fn is_direct_deref_use_rvalue(e: &str) -> bool {
    DIRECT_DEREF_USE_RE.is_match(e.trim())
}

fn is_closure_aggregate_rvalue(e: &str) -> bool {
    e.trim().starts_with("{closure@")
}

/// Return the MIR function scope encoded in a GlobalICFG MIR node id.
///
/// `rust::main::{closure#0}::bb3` -> `rust::main::{closure#0}`
fn mir_function_scope_from_node_id(node_id: &str) -> Option<String> {
    let (scope, bb) = node_id.rsplit_once("::bb")?;
    if !bb.is_empty() && bb.chars().all(|c| c.is_ascii_digit()) {
        Some(scope.to_string())
    } else {
        None
    }
}

/// Resolve a direct-reference chain within one MIR function scope to its leaf
/// stack places.  Returning an empty set means that `local` is not known to be
/// a stack-reference local.
fn resolve_scoped_stack_ref_leaves(
    scope: &str,
    local: &Name,
    refs: &BTreeMap<(String, Name), BTreeSet<Name>>,
) -> BTreeSet<Name> {
    let start = canonical_mir_local(local);
    let start_key = (scope.to_string(), start.clone());

    if !refs.contains_key(&start_key) {
        return BTreeSet::new();
    }

    let mut leaves = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut work = vec![start];

    while let Some(current) = work.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }

        let key = (scope.to_string(), current.clone());
        if let Some(nexts) = refs.get(&key) {
            for next in nexts {
                work.push(canonical_mir_local(next));
            }
        } else {
            leaves.insert(current);
        }
    }

    leaves
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ClosureCaptureBinding {
    /// The captured field stores a reference to these outer stack places.
    stack_ref_targets: BTreeSet<Name>,
    /// The captured field stores these values directly.
    by_value_sources: BTreeSet<Name>,
}

fn merge_closure_capture_vectors(
    dst: &mut Vec<ClosureCaptureBinding>,
    src: &[ClosureCaptureBinding],
) {
    if dst.len() < src.len() {
        dst.resize(src.len(), ClosureCaptureBinding::default());
    }

    for (idx, item) in src.iter().enumerate() {
        dst[idx]
            .stack_ref_targets
            .extend(item.stack_ref_targets.iter().cloned());
        dst[idx]
            .by_value_sources
            .extend(item.by_value_sources.iter().cloned());
    }
}

/// Find the actual closure entry reached from one closure call site using the
/// GlobalICFG edges inserted by `icfg.rs`:
///
/// call-site -> dummyCall -> rust::<...{closure#N}>::bb0
fn closure_entry_scope_for_call(
    icfg: &GlobalICFGOrdered,
    call_node_id: &str,
) -> Option<String> {
    let dummy = icfg
        .icfg_edges
        .iter()
        .find(|e| {
            e.source == call_node_id
                && e.destination.starts_with("dummyCall")
                && e.label
                    .as_deref()
                    .map(|l| l.contains("Closure Call"))
                    .unwrap_or(false)
        })?
        .destination
        .clone();

    let entry = icfg
        .icfg_edges
        .iter()
        .find(|e| {
            e.source == dummy
                && e.destination.starts_with("rust::")
                && e.destination.contains("{closure#")
                && e.destination.ends_with("::bb0")
        })?
        .destination
        .clone();

    mir_function_scope_from_node_id(&entry)
}

/// Precompute closure-capture semantics independently from detector traversal
/// order.
///
/// This is deliberately separate from heap aliasing:
/// - `&_2` records stack-reference flow;
/// - `{closure...} { ptr: move _4 }` records what the closure field stores;
/// - the closure call ICFG edges identify which closure body receives that
///   environment.
fn build_closure_capture_bindings(
    icfg: &GlobalICFGOrdered,
) -> BTreeMap<String, Vec<ClosureCaptureBinding>> {
    let mut scoped_refs: BTreeMap<(String, Name), BTreeSet<Name>> =
        BTreeMap::new();

    // Pass 1: direct Ref/AddressOf relations within each MIR function.
    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else {
            continue;
        };
        let Some(scope) = mir_function_scope_from_node_id(node_id) else {
            continue;
        };

        for stmt in &bb.statements {
            let (Some(dest), Some(rvalue)) = (&stmt.place, &stmt.rvalue) else {
                continue;
            };

            if let Some(src) = direct_stack_borrow_source(rvalue) {
                scoped_refs
                    .entry((scope.clone(), canonical_mir_local(dest)))
                    .or_default()
                    .insert(canonical_mir_local(&src));
            }
        }
    }

    // Pass 2: closure environment locals -> ordered field capture bindings.
    let mut envs: BTreeMap<(String, Name), Vec<ClosureCaptureBinding>> =
        BTreeMap::new();

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else {
            continue;
        };
        let Some(scope) = mir_function_scope_from_node_id(node_id) else {
            continue;
        };

        for stmt in &bb.statements {
            let (Some(dest), Some(rvalue)) = (&stmt.place, &stmt.rvalue) else {
                continue;
            };

            let Some(operands) = closure_aggregate_capture_operands(rvalue) else {
                continue;
            };

            let mut captures = Vec::with_capacity(operands.len());
            for operand in operands {
                let leaves =
                    resolve_scoped_stack_ref_leaves(&scope, &operand, &scoped_refs);

                let mut binding = ClosureCaptureBinding::default();
                if leaves.is_empty() {
                    binding
                        .by_value_sources
                        .insert(canonical_mir_local(&operand));
                } else {
                    binding.stack_ref_targets.extend(leaves);
                }
                captures.push(binding);
            }

            envs.insert(
                (scope.clone(), canonical_mir_local(dest)),
                captures,
            );
        }
    }

    // Pass 3: map every canonical closure call relation to the closure body.
    //
    // Older code rediscovered direct Fn/FnMut/FnOnce calls from MIR debug text.
    // v6L higher-order summaries create the same canonical `rust_calls` relation
    // for callbacks invoked by external library semantics.  Consuming this
    // relation here makes capture recovery independent of whether the call edge
    // came directly from a MIR Fn-call terminator or from a sound library
    // summary.
    let mut by_closure_scope: BTreeMap<
        String,
        Vec<ClosureCaptureBinding>,
    > = BTreeMap::new();

    for call in &icfg.rust_calls {
        if !call.is_closure {
            continue;
        }
        let Some(first_arg) = call.arguments.first() else {
            // Zero-capture closures need no environment binding.
            continue;
        };

        // RustCallMetadata::caller_function stores the rustc DefPath without
        // the GlobalICFG `rust::` node-id prefix.  Capture environments above
        // are indexed by the canonical MIR node scope (e.g. `rust::main`).
        // Derive the lookup scope from the canonical call node itself rather
        // than mixing these two namespaces.
        let Some(scope) = mir_function_scope_from_node_id(&call.call_node) else {
            continue;
        };
        let arg = canonical_mir_local(&first_arg.arg);
        let mut env_candidates =
            resolve_scoped_stack_ref_leaves(&scope, &arg, &scoped_refs);

        if env_candidates.is_empty() {
            env_candidates.insert(arg);
        }

        let closure_scope = call.callee_function.clone();
        for env_local in env_candidates {
            if let Some(captures) =
                envs.get(&(scope.clone(), canonical_mir_local(&env_local)))
            {
                merge_closure_capture_vectors(
                    by_closure_scope
                        .entry(closure_scope.clone())
                        .or_default(),
                    captures,
                );
            }
        }
    }

    by_closure_scope
}

fn is_heap_dependent_value(v: CellValue) -> bool {
    matches!(
        v,
        CellValue::ALLOC
            | CellValue::FREED
            | CellValue::MB
            | CellValue::IMMB
            | CellValue::MV
            | CellValue::TOP
    )
}

/// Abstract evaluation of a MIR Rvalue for the memory-management domain.
///
/// Implemented formal/core cases:
/// - scalar/unit constants -> BOXTIMES;
/// - direct copy/move of a local -> abstract value of that local;
/// - scalar casts and scalar-producing nullary ops -> BOXTIMES;
/// - unary/binary scalar operations follow the formal BOXTIMES/TOP/BOTTOM rule.
///
/// Unknown Rust-specific aggregate/projection forms deliberately retain the old
/// sparse fallback in this phase. See PHASE3_BOTTOM_TOP_AUDIT.md.
pub fn eval_rvalue(e: &str, sigma: &AbstractMemory) -> CellValue {
    use CellValue::*;

    let trimmed = e.trim();

    if is_scalar_const_rvalue(trimmed)
        || is_scalar_nullary_rvalue(trimmed)
        || scalar_cast_target(trimmed).is_some()
    {
        return BOXTIMES;
    }

    // Closure aggregates and CopyForDeref are valid represented MIR values,
    // but their field/content value is not expressible in the current
    // non-field-sensitive CellValue domain. TOP is conservative; BOTTOM would
    // incorrectly mean that no normal value is represented.
    if is_closure_aggregate_rvalue(trimmed)
        || is_copy_for_deref_rvalue(trimmed)
        || is_direct_deref_use_rvalue(trimmed)
    {
        return TOP;
    }

    if let Some(local) = exact_local_operand(trimmed, "copy ") {
        return sigma.get_cell_value(&full_local_name(&local));
    }

    if let Some(local) = exact_local_operand(trimmed, "move ") {
        return sigma.get_cell_value(&full_local_name(&local));
    }

    // Backward-compatible textual forms already used by the analyzer.
    if trimmed.starts_with("& imm") {
        return IMMB;
    }
    if trimmed.starts_with("& mut") {
        return MB;
    }

    // MIR comparison operations return bool even when comparing raw pointers.
    // The result is therefore a scalar BOXTIMES whenever both operands denote
    // a represented value. BOTTOM still means no represented normal value.
    if let Some((lhs, rhs)) = comparison_rvalue_args(trimmed) {
        let lhs = eval_rvalue(lhs, sigma);
        let rhs = eval_rvalue(rhs, sigma);

        return if lhs == BOTTOM || rhs == BOTTOM {
            BOTTOM
        } else {
            BOXTIMES
        };
    }

    if let Some((lhs, rhs)) = binary_rvalue_args(trimmed) {
        let lhs = eval_rvalue(lhs, sigma);
        let rhs = eval_rvalue(rhs, sigma);

        return if lhs == TOP || rhs == TOP {
            TOP
        } else if lhs == BOXTIMES && rhs == BOXTIMES {
            BOXTIMES
        } else {
            BOTTOM
        };
    }

    if let Some(arg) = unary_rvalue_arg(trimmed) {
        return match eval_rvalue(arg, sigma) {
            TOP => TOP,
            BOXTIMES => BOXTIMES,
            _ => BOTTOM,
        };
    }

    // Phase-2 compatibility for MIR outside the positively recognized core.
    sigma.get_cell_value(&trimmed.to_string())
}

// Helper to extract only the variable name string from an argument like "Local(_2) [mutable]"
fn extract_arg_name(arg: &str) -> String {
    if let Some(start) = arg.find('(') {
        if let Some(end) = arg.find(')') {
            return arg[start + 1..end].to_string();
        }
    }
    arg.to_string()
}



// ----------------------------------------------------------------------
// v6P MIR semantic-extension transfer layer
// ----------------------------------------------------------------------
//
// Scientific boundary:
// * `apply_mir_statement` below is the frozen legacy transfer and is not
//   modified by v6P.
// * this adapter is opt-in (`--mir-semantics-v2`), handles only cases that the
//   frozen classifier calls `unmodeled`, and otherwise delegates byte-for-byte
//   to the legacy transfer;
// * no new CellValue element or order edge is introduced.
//
// For an unmodeled assignment, TOP is the conservative abstraction of the
// produced value.  This intentionally sacrifices precision rather than
// manufacturing ALLOC/FREED/borrow facts. StorageLive/StorageDead follow the
// formal distinction between uninitialized stack storage (BOXTIMES) and an
// absent stack local (BOTTOM); Deinit exact locals map to BOXTIMES.
fn v2_overwrite_destination(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
    value: CellValue,
) -> Option<AbstractMemory> {
    let dest = stmt.place.as_ref()?;
    let dest_key = full_local_name(dest);
    let mut new_mem = mem.clone();
    new_mem.assign_local_value(&dest_key, value);
    taint.remove(&dest_key);
    Some(new_mem)
}

/// Transfer for a MIR `Assign` whose Rvalue was not positively covered by the
/// frozen v6O transfer.  Every branch is either justified by the MIR value
/// contract or deliberately widens to TOP; it never manufactures allocation,
/// deallocation, ownership, or borrow facts.
fn transfer_unmodeled_assign_v2(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
) -> Option<AbstractMemory> {
    let rvalue = stmt.rvalue.as_deref()?;
    let category = rvalue_category(rvalue);

    let value = match category {
        // Rvalue::Use evaluates one Operand.  Reuse the frozen evaluator when
        // it can positively recover a represented value.  A sparse/projection
        // fallback to BOTTOM is *not* evidence that the runtime value is
        // absent, so widen that case to TOP.
        "use" | "const" => {
            let evaluated = eval_rvalue(rvalue, mem);
            if evaluated == CellValue::BOTTOM { CellValue::TOP } else { evaluated }
        }

        // Discriminant and Len are integer/scalar results. Checked integer
        // arithmetic produces a pair `(value, overflow)`; for the allocation
        // domain every component is non-owning scalar data, so BOXTIMES is the
        // precise abstraction needed by CellValue (no field sensitivity is
        // claimed here).
        "discriminant" | "len" | "nullary_op" | "checked_binary_op" => {
            CellValue::BOXTIMES
        }

        // Pointer metadata may itself be pointer-shaped (e.g. vtable metadata)
        // and aggregates can contain arbitrary operands.  CellValue has no
        // representation precise enough to distinguish those cases, therefore
        // the sound extension is TOP rather than a fabricated scalar/heap fact.
        "ptr_metadata"
        | "aggregate"
        | "repeat"
        | "thread_local_ref"
        | "shallow_init_box"
        | "copy_for_deref"
        | "address_of"
        | "ref"
        | "cast"
        | "binary_op"
        | "unary_op"
        | "other" => CellValue::TOP,

        // `rvalue_category` is a closed v1 adapter.  Keep this wildcard so a
        // future vocabulary extension remains fail-safe rather than silently
        // acquiring a precise transfer.
        _ => CellValue::TOP,
    };

    v2_overwrite_destination(mem, taint, stmt, value)
}

/// Return the canonical root local for producer-recorded statement places.
/// v6P-r1d deliberately consumes `MirStatement::place`, which is emitted from
/// the typed rustc MIR place, rather than reparsing `details` Debug text.
fn statement_place_key(stmt: &MirStatement) -> Option<Name> {
    stmt.place.as_deref().map(full_local_name)
}

fn statement_place_is_projected(stmt: &MirStatement) -> bool {
    stmt.place
        .as_deref()
        .map(|p| p.contains(" -> "))
        .unwrap_or(false)
}

/// Fail-safe havoc for effects that the current CellValue domain cannot
/// localize.  TOP is the greatest element of the unchanged lattice, so this is
/// an over-approximation and cannot manufacture ALLOC/FREED/borrow facts.
fn widen_all_tracked_to_top(mem: &AbstractMemory) -> AbstractMemory {
    let mut out = mem.clone();
    let vars: Vec<Name> = out.all_vars().into_iter().collect();
    for var in vars {
        out.set_cell_value(&var, CellValue::TOP);
    }
    out
}

/// `StorageLive(local)` allocates fresh *uninitialized* stack storage in the
/// pinned rustc MIR semantics.  The formal stack abstraction maps an existing
/// uninitialized local to BOXTIMES; BOTTOM is reserved for an absent/unallocated
/// local.  Repeated StorageLive also replaces prior storage with fresh uninit.
fn transfer_storage_live_v2(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
) -> Option<AbstractMemory> {
    let key = statement_place_key(stmt)?;
    let mut new_mem = mem.clone();
    new_mem.assign_local_value(&key, CellValue::BOXTIMES);
    taint.remove(&key);
    Some(new_mem)
}

/// `StorageDead(local)` deallocates the stack slot (or is a NOP if already
/// dead).  In the formal stack abstraction an unallocated local is outside the
/// stack domain and therefore maps to BOTTOM.  This is *not* a heap free.
fn transfer_storage_dead_v2(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
) -> Option<AbstractMemory> {
    let key = statement_place_key(stmt)?;
    let mut new_mem = mem.clone();
    new_mem.assign_local_value(&key, CellValue::BOTTOM);
    taint.remove(&key);
    Some(new_mem)
}

/// MIR `Deinit(place)` writes uninitialized bytes to the entire place without
/// executing its destructor.  Exact locals therefore abstract to BOXTIMES.
/// For projections the current domain is not field-sensitive: changing only a
/// sub-place cannot be represented, so widen the root allocation component to
/// TOP.  Missing producer place evidence falls back to global TOP rather than
/// under-approximating.
fn transfer_deinit_v2(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
) -> Option<AbstractMemory> {
    let Some(key) = statement_place_key(stmt) else {
        return Some(widen_all_tracked_to_top(mem));
    };
    let mut new_mem = mem.clone();
    if statement_place_is_projected(stmt) {
        new_mem.set_cell_value(&key, CellValue::TOP);
    } else {
        new_mem.assign_local_value(&key, CellValue::BOXTIMES);
        taint.remove(&key);
    }
    Some(new_mem)
}

/// A statement that mutates a typed MIR place in a way not expressible in the
/// CellValue lattice conservatively havocs the represented allocation component
/// to TOP.  This is used for SetDiscriminant and Retag.
fn transfer_opaque_place_write_v2(
    mem: &AbstractMemory,
    stmt: &MirStatement,
) -> AbstractMemory {
    let Some(key) = statement_place_key(stmt) else {
        return widen_all_tracked_to_top(mem);
    };
    let mut new_mem = mem.clone();
    new_mem.set_cell_value(&key, CellValue::TOP);
    new_mem
}

/// Pinned `NonDivergingIntrinsic` has two variants.
///
/// * Assume only restricts feasible executions. Retaining the current abstract
///   state even when the condition is false is a standard MAY over-approximation
///   (we do not claim path pruning precision).
/// * CopyNonOverlapping writes through `dst`.  Without a complete points-to
///   proof, changing only the pointer operand would miss possible aliases, so
///   every tracked CellValue is widened to TOP.
fn transfer_intrinsic_v2(mem: &AbstractMemory, stmt: &MirStatement) -> AbstractMemory {
    if stmt.details.starts_with("Intrinsic::Assume ") {
        return mem.clone();
    }
    if stmt.details.starts_with("Intrinsic::CopyNonOverlapping ") {
        // The destination is reached through a raw/reference/Box operand.  The
        // present domain has allocation identity but no complete points-to
        // relation for arbitrary bytewise writes, so localizing the mutation
        // to the pointer operand would be unsound.  Global TOP preserves every
        // possible heap-state effect without inventing one.
        return widen_all_tracked_to_top(mem);
    }
    widen_all_tracked_to_top(mem)
}

/// Statements documented by the pinned rustc semantics as runtime no-ops (or
/// metadata-only operations with no CellValue memory effect) preserve the
/// abstract memory.  This helper is intentionally not used for SetDiscriminant,
/// Retag, or CopyNonOverlapping.
fn transfer_runtime_noop_statement_v2(mem: &AbstractMemory) -> AbstractMemory {
    mem.clone()
}

fn apply_mir_statement_v2_extension(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
) -> Option<AbstractMemory> {
    if !mir_semantics_v2_enabled() || classify_mir_statement_coverage(stmt) != "unmodeled" {
        return None;
    }

    match stmt.kind.as_str() {
        "Assign" => transfer_unmodeled_assign_v2(mem, taint, stmt),
        "StorageLive" => transfer_storage_live_v2(mem, taint, stmt),
        "StorageDead" => transfer_storage_dead_v2(mem, taint, stmt),
        "Deinit" => transfer_deinit_v2(mem, taint, stmt),
        "SetDiscriminant" | "Retag" => Some(transfer_opaque_place_write_v2(mem, stmt)),
        "Intrinsic" => Some(transfer_intrinsic_v2(mem, stmt)),
        "FakeRead"
        | "PlaceMention"
        | "AscribeUserType"
        | "Coverage"
        | "ConstEvalCounter"
        | "BackwardIncompatibleDropHint" => Some(transfer_runtime_noop_statement_v2(mem)),
        _ => None,
    }
}

pub fn apply_mir_statement_with_extensions(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    stmt: &MirStatement,
) -> AbstractMemory {
    if let Some(updated) = apply_mir_statement_v2_extension(mem, taint, stmt) {
        updated
    } else {
        apply_mir_statement(mem, taint, stmt)
    }
}

/// v6P-r1d terminator extension. The frozen legacy transfer remains byte-
/// identical and is used for all previously represented cases.  Terminators
/// whose memory effect is not representable are supported by widening rather
/// than by silently treating them as no-ops.
pub fn apply_mir_terminator_with_extensions(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    term: &MirTerminator,
) -> AbstractMemory {
    if mir_semantics_v2_enabled() {
        match term {
            // Inline assembly may read/write arbitrary memory according to its
            // operands/options.  Until those effects are typed into CellValue,
            // global TOP is the only sound memory-state abstraction.
            MirTerminator::InlineAsm { .. } => return widen_all_tracked_to_top(mem),
            // Yield transfers control out of the current coroutine invocation
            // and later writes a resume argument.  The current intraprocedural
            // state domain has no coroutine-frame abstraction, so widen before
            // propagating to the MAY resume/drop continuations.
            MirTerminator::Yield { .. } => return widen_all_tracked_to_top(mem),
            _ => {}
        }
    }
    apply_mir_terminator(mem, taint, term)
}

/// Coverage view for the opt-in extension profile.  The legacy classifier is
/// intentionally retained as a separate function so v6O evidence remains
/// reproducible and auditable.
pub fn classify_mir_statement_coverage_v2(stmt: &MirStatement) -> &'static str {
    let legacy = classify_mir_statement_coverage(stmt);
    if !mir_semantics_v2_enabled() || legacy != "unmodeled" {
        return legacy;
    }

    match stmt.kind.as_str() {
        "Assign" => match stmt.rvalue.as_deref().map(rvalue_category) {
            Some("discriminant" | "len" | "nullary_op" | "checked_binary_op") => "precise",
            Some(_) => "conservative",
            None => "unmodeled",
        },
        "StorageLive" | "StorageDead" => "precise",
        "Deinit" => {
            if statement_place_is_projected(stmt) { "conservative" } else { "precise" }
        }
        "FakeRead"
        | "PlaceMention"
        | "AscribeUserType"
        | "Coverage"
        | "ConstEvalCounter"
        | "BackwardIncompatibleDropHint" => "precise",
        // These statements are supported but deliberately widened because the
        // current CellValue domain does not represent their full effect.
        "SetDiscriminant" | "Retag" | "Intrinsic" => "conservative",
        _ => "unmodeled",
    }
}

// ----------------------------------------------------------------------
// MIR STATEMENT/TERMINATOR DISPATCH
// ----------------------------------------------------------------------
// process an entire MIR basic block (which may consist of zero or more statements followed by a terminator)
// returns a tuple of the updated AbstractMemory and TaintStateMap
pub fn process_mir_basic_block(block: &MirBasicBlock, init_mem: &AbstractMemory, init_taint: &TaintStateMap) -> (AbstractMemory, TaintStateMap) {
    let mut current_mem = init_mem.clone();
    let mut current_taint = init_taint.clone();
    for stmt in &block.statements {
        current_mem = apply_mir_statement_with_extensions(&current_mem, &mut current_taint, stmt);
    }

    // A3 panic/unwind profile: terminator effects are edge-sensitive.  Applying
    // them here would collapse the normal and unwind continuations back into
    // one post-state, which is precisely the unsoundness this profile removes.
    // The frozen/default profile keeps the historical node-local transfer.
    if !panic_unwind_lifecycle_v1_enabled() {
        if let Some(term) = &block.terminator {
            current_mem = apply_mir_terminator_with_extensions(&current_mem, &mut current_taint, term);
        }
    }
    (current_mem, current_taint)
}

/// Conservative unwind-side effect for a MIR call whose body is not represented
/// on the current edge.  A call that unwinds does not assign its destination,
/// while memory reachable through its arguments may have been partially
/// mutated/dropped before the panic.  CellValue has no partial-lifecycle state,
/// so TOP is the sound projection until a richer lifecycle domain is added.
fn apply_call_unwind_effect(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    arguments: &[crate::structs::MirCallArgument],
    return_place: &str,
) -> AbstractMemory {
    let mut out = mem.clone();

    if !return_place.trim().is_empty() {
        let ret = full_local_name(return_place);
        out.assign_local_value(&ret, CellValue::BOTTOM);
        taint.remove(&ret);
    }

    for arg in arguments {
        let raw = extract_arg_name(&arg.arg);
        if raw.is_empty() {
            continue;
        }
        let name = full_local_name(&raw);
        if out.get_allocation(&name).is_some() {
            out.set_cell_value(&name, CellValue::TOP);
        }
        taint
            .entry(name)
            .or_default()
            .insert("unwind_may_effect".to_string());
    }

    out
}

/// Edge-sensitive MIR terminator transfer used only by
/// `panic_unwind_lifecycle_v1`.
///
/// Normal edges preserve the existing transfer semantics.  Unwind edges do
/// *not* reuse the normal post-state: calls suppress return-place assignment,
/// Drops become partial/unknown rather than definitely FREED, and assertions
/// leave memory unchanged because the panic occurs before their success
/// continuation.
fn apply_mir_terminator_for_edge_enabled(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    term: &MirTerminator,
    edge: &IcfgEdge,
) -> AbstractMemory {
    // Entering a represented callee is neither a normal *return* nor an unwind
    // of the Call terminator.  The callee body owns its effects and the
    // DummyRet node maps `_0` into the caller return place on successful return.
    // Applying the Call transfer here would manufacture a return value before
    // the callee executes.  The same rule applies to represented FFI bodies.
    if matches!(
        edge.label.as_deref(),
        Some("Rust Call -> dummyCall")
            | Some("Rust Drop -> dummyCall")
            | Some("Higher-order callback -> dummyCall")
            | Some("FFI Call")
    ) {
        return mem.clone();
    }

    match edge_flow_kind(edge) {
        EdgeFlowKind::Normal => apply_mir_terminator_with_extensions(mem, taint, term),
        EdgeFlowKind::Unwind => match term {
            MirTerminator::Call { arguments, return_place, .. } => {
                apply_call_unwind_effect(mem, taint, arguments, return_place)
            }
            MirTerminator::Drop { dropped_value, .. } => {
                let mut out = mem.clone();
                let dropped = full_local_name(dropped_value);
                if out.get_allocation(&dropped).is_some() {
                    out.set_cell_value(&dropped, CellValue::TOP);
                }
                taint
                    .entry(dropped)
                    .or_default()
                    .insert("unwind_partial_drop".to_string());
                out
            }
            MirTerminator::Assert { .. } => mem.clone(),
            MirTerminator::UnwindResume { .. } => mem.clone(),
            MirTerminator::InlineAsm { .. } => widen_all_tracked_to_top(mem),
            // Defensive fallback: an edge classified as unwind for a future
            // terminator kind must not silently acquire normal-return effects.
            _ => widen_all_tracked_to_top(mem),
        },
    }
}

pub fn apply_mir_terminator_for_edge_with_extensions(
    mem: &AbstractMemory,
    taint: &mut TaintStateMap,
    term: &MirTerminator,
    edge: &IcfgEdge,
) -> AbstractMemory {
    if !panic_unwind_lifecycle_v1_enabled() {
        return apply_mir_terminator_with_extensions(mem, taint, term);
    }
    apply_mir_terminator_for_edge_enabled(mem, taint, term, edge)
}

// ----------------------------------------------------------------------
// MIR TRANSFER FUNCTIONS
// ----------------------------------------------------------------------
// CALL TERMINATOR: transfer_call simulates the effect of a MIR FUNCTION CALL on the abstract memory
// It returns the CellValue for the call’s return place and the updated memory
// TO DO: CHECK THAT IF MATCHES THE ALLCO SLICE CASE, DOES NOT TO CHECK THE BOX::NEW CASE

// NB
/* non esiste la box into raw chiamata sui vectors, è possibile solo convertendola prima uin una cstring
e pi su quella cstring fare una box into raw per passarla a c
 */

////////////////////////////////////////////////////////////////////////////////
// ALLOC regex
static BOX_NEW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<[^>]+>::new").unwrap()
});


static BOX_VEC_NEW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<std::vec::Vec<[^>]+>>::new").unwrap()
});

static VEC_ALLOC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Box::<Vec<[^>]+>::new").unwrap()
});


static BOX_NEW_NODE_GENERIC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(std::boxed::)?Box::<Node<[^>]+>>::new").unwrap()
});
////////////////////////////////////////////////////////////////////////////////

//mem forget regex
static VEC_INTO_RAW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Box::<Vec<[^>]+>>::into_raw").unwrap()
});


static BOX_INTO_RAW_NODE_GENERIC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(std::boxed::)?Box::<Node<[^>]+>>::into_raw").unwrap()
});
////////////////////////////////////////////////////////////////////////////////
//from raw regex
static FROM_RAW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<[^>]+>::from_raw").unwrap()
});

static BOX_VEC_FROM_RAW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<std::vec::Vec<[^>]+>>::from_raw").unwrap()
});

static VEC_FROM_RAW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<std::vec::Vec<[^>]+>>::from_raw").unwrap()
});

static BOX_FROM_RAW_NODE_GENERIC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(std::boxed::)?Box::<Node<[^>]+>>::from_raw").unwrap()
});
//////////////////////////////////////////////////////////////////////////////////
// Standard-library memory effects added in Phase 4.
//
// These predicates are deliberately semantic rather than type-enumeration based.
// The old type-specific matchers remain below for backward compatibility with
// the published corpus, while the new cases cover generic standard APIs whose
// documented ownership/deallocation effect is type-independent.

fn is_box_new_call(s: &str) -> bool {
    (s.contains("std::boxed::Box::<") || s.contains("alloc::boxed::Box::<"))
        && s.contains(">::new")
}

fn is_box_into_raw_call(s: &str) -> bool {
    (s.contains("std::boxed::Box::<") || s.contains("alloc::boxed::Box::<"))
        && s.contains(">::into_raw")
}

fn is_box_from_raw_call(s: &str) -> bool {
    (s.contains("std::boxed::Box::<") || s.contains("alloc::boxed::Box::<"))
        && s.contains(">::from_raw")
}

fn is_cstring_into_raw_call(s: &str) -> bool {
    s.contains("std::ffi::CString::into_raw")
        || s.contains("alloc::ffi::c_str::<impl std::ffi::CString>::into_raw")
}

fn is_cstring_from_raw_call(s: &str) -> bool {
    s.contains("std::ffi::CString::from_raw")
        || s.contains("alloc::ffi::c_str::<impl std::ffi::CString>::from_raw")
}

fn is_owning_into_raw_call(s: &str) -> bool {
    is_box_into_raw_call(s) || is_cstring_into_raw_call(s)
}

fn is_owning_from_raw_call(s: &str) -> bool {
    is_box_from_raw_call(s) || is_cstring_from_raw_call(s)
}

fn is_mem_forget_call(s: &str) -> bool {
    s.contains("std::mem::forget::<") || s.contains("core::mem::forget::<")
}

fn is_box_leak_call(s: &str) -> bool {
    (s.contains("std::boxed::Box::<") || s.contains("alloc::boxed::Box::<"))
        && s.contains(">::leak")
}

fn is_vec_from_raw_parts_call(s: &str) -> bool {
    (s.contains("std::vec::Vec::<") || s.contains("alloc::vec::Vec::<"))
        && s.contains(">::from_raw_parts")
}

fn is_string_from_raw_parts_call(s: &str) -> bool {
    s.contains("std::string::String::from_raw_parts")
        || s.contains("alloc::string::String::from_raw_parts")
}

fn is_raw_alloc_zeroed_call(s: &str) -> bool {
    s.contains("std::alloc::alloc_zeroed")
        || s.contains("alloc::alloc::alloc_zeroed")
}

fn is_raw_alloc_call(s: &str) -> bool {
    (s.contains("std::alloc::alloc") || s.contains("alloc::alloc::alloc"))
        && !is_raw_alloc_zeroed_call(s)
        && !s.contains("handle_alloc_error")
}

fn is_raw_dealloc_call(s: &str) -> bool {
    s.contains("std::alloc::dealloc") || s.contains("alloc::alloc::dealloc")
}

fn is_raw_realloc_call(s: &str) -> bool {
    memory_events::is_raw_realloc_call(s)
}

fn is_cstr_from_ptr_call(s: &str) -> bool {
    s.contains("std::ffi::CStr::from_ptr")
        || s.contains("core::ffi::CStr::from_ptr")
        || s.contains("ffi::c_str::<impl std::ffi::CStr>::from_ptr")
}


/// Raw-pointer method cast emitted as a MIR Call terminator by the pinned
/// toolchain, e.g.
/// `std::ptr::mut_ptr::<impl *mut i32>::cast::<std::ffi::c_void>`.
/// This is an address-preserving view conversion, not an ownership transfer.
fn is_raw_pointer_cast_method_call(s: &str) -> bool {
    let mut_ptr = s.contains("::ptr::mut_ptr::<impl *mut ");
    let const_ptr = s.contains("::ptr::const_ptr::<impl *const ");
    (mut_ptr || const_ptr) && s.contains(">::cast::<")
}

/// APIs that expose a raw pointer borrowed from an existing owner/view without
/// transferring ownership. We keep this type-filtered: `MaybeUninit::as_mut_ptr`
/// for example points into a stack/local object and must not be confused with
/// a tracked heap allocation.
fn is_borrowed_raw_pointer_view_call(s: &str) -> bool {
    if is_raw_pointer_cast_method_call(s) {
        return true;
    }

    let as_ptr = s.contains("::as_ptr") || s.contains("::as_mut_ptr");
    as_ptr
        && (s.contains("CString")
            || s.contains("CStr")
            || s.contains("Vec::<")
            || s.contains("String")
            || s.contains("NonNull::<")
            || s.contains("str>::as_ptr")
            || s.contains("slice::<impl ["))
}

fn is_ptr_read_call(s: &str) -> bool {
    s.contains("std::ptr::read::<")
        || s.contains("core::ptr::read::<")
        || s.contains("std::ptr::read_unaligned::<")
        || s.contains("core::ptr::read_unaligned::<")
        || s.contains("std::ptr::read_volatile::<")
        || s.contains("core::ptr::read_volatile::<")
}

fn is_ptr_write_call(s: &str) -> bool {
    s.contains("std::ptr::write::<")
        || s.contains("core::ptr::write::<")
        || s.contains("std::ptr::write_unaligned::<")
        || s.contains("core::ptr::write_unaligned::<")
        || s.contains("std::ptr::write_volatile::<")
        || s.contains("core::ptr::write_volatile::<")
}

fn is_ptr_drop_in_place_call(s: &str) -> bool {
    s.contains("std::ptr::drop_in_place::<")
        || s.contains("core::ptr::drop_in_place::<")
}

fn is_pointer_memory_use_call(s: &str) -> bool {
    is_ptr_read_call(s) || is_ptr_write_call(s) || is_ptr_drop_in_place_call(s)
}

/// Return the first direct MIR local operand (`move _N` or `copy _N`) that
/// appears in the call-argument portion of a textual MIR Call.
///
/// This intentionally does not reuse `extract_moved_var` / `extract_copied_var`:
/// those historical helpers split on whitespace and therefore keep punctuation
/// such as the comma in `copy _1, copy _2`, yielding the invalid name `_1,`.
///
/// We select whichever of `move`/`copy` occurs first, then use LOCAL_TOKEN_RE so
/// punctuation never becomes part of the local name.
fn first_call_local_from_details(details: &str) -> Option<Name> {
    let call_part = details.split("->").next().unwrap_or(details);

    let move_pos = call_part.find("move ");
    let copy_pos = call_part.find("copy ");

    let (start, keyword_len) = match (move_pos, copy_pos) {
        (Some(m), Some(c)) if m <= c => (m, "move ".len()),
        (Some(_), Some(c)) => (c, "copy ".len()),
        (Some(m), None) => (m, "move ".len()),
        (None, Some(c)) => (c, "copy ".len()),
        (None, None) => return None,
    };

    let operand = &call_part[start + keyword_len..];
    let local = LOCAL_TOKEN_RE.find(operand)?;

    if local.start() == 0 {
        Some(full_local_name(local.as_str()))
    } else {
        None
    }
}

fn leak_ghost_name(source: &Name) -> Name {
    format!("Leak({})", normalize_name(source))
}

fn replace_consumed_local_with(
    mem: &mut AbstractMemory,
    source: &Name,
    replacement: Name,
    value: CellValue,
) {
    if let Some(old_alloc) = mem.get_allocation(source) {
        mem.state.remove(&old_alloc);
        let mut set = old_alloc.set;
        set.remove(source);
        set.insert(replacement);
        mem.state.insert(Allocation { set }, value);
    } else {
        mem.state.insert(Allocation::new(replacement), value);
    }
}

//////////////////////////////////////////////////////////////////////////////////

pub fn transfer_call(mem: &AbstractMemory, func_call_details: &str, return_place: &str) -> (CellValue, AbstractMemory) {
    let ffi_functions = match load_ffi_functions("./ffi_functions.json") {
        Ok(set) => set,
        Err(e) => {
            eprintln!("Failed to load FFI functions: {:?}", e);
            std::collections::HashSet::new()
        }
    };

    // ALLOC 
    let mut new_mem = mem.clone();
    let ret_val = if is_box_new_call(func_call_details) {
        // Box::new creates a fresh owning allocation. A fresh return local must
        // not be merged with another allocation merely because both are ALLOC.
        let full_ret = full_local_name(return_place);
        new_mem.assign_local_value(&full_ret, CellValue::ALLOC);
        CellValue::ALLOC

    } else if is_owning_into_raw_call(func_call_details) {
        // Box/CString into_raw consume the owner and return a pointer to the
        // SAME allocation. Preserve any pre-existing aliases while replacing
        // the consumed owner local with the returned raw handle.
        let full_ret = full_local_name(return_place);
        if let Some(source) = first_call_local_from_details(func_call_details) {
            replace_consumed_local_with(
                &mut new_mem,
                &source,
                full_ret,
                CellValue::MV,
            );
        } else {
            new_mem.assign_local_value(&full_ret, CellValue::MV);
        }
        CellValue::MV

    } else if is_owning_from_raw_call(func_call_details) {
        // from_raw restores an owning handle to the same allocation. Keep the
        // raw local as an alias: raw pointers are Copy and reusing it can be a
        // real double-free/UAF source. The one-value-per-alias-component
        // representation remains MV until the domain is split by handle kind.
        let full_ret = full_local_name(return_place);
        if let Some(source) = first_call_local_from_details(func_call_details) {
            if let Some(old_alloc) = new_mem.get_allocation(&source) {
                new_mem.state.remove(&old_alloc);
                let mut set = old_alloc.set;
                set.insert(full_ret.clone());
                new_mem.state.insert(Allocation { set }, CellValue::MV);
            } else {
                new_mem.assign_local_value(&full_ret, CellValue::MV);
            }
        } else {
            new_mem.assign_local_value(&full_ret, CellValue::MV);
        }
        CellValue::MV

    } else if is_mem_forget_call(func_call_details) {
        // mem::forget consumes its argument and returns ().  It suppresses
        // destructor execution; if the argument owns a tracked allocation,
        // keep a synthetic allocation witness so leak provenance survives even
        // though the source local itself has been moved.
        if let Some(source) = first_call_local_from_details(func_call_details) {
            if new_mem.get_allocation(&source).is_some() {
                let ghost = leak_ghost_name(&source);
                replace_consumed_local_with(
                    &mut new_mem,
                    &source,
                    ghost,
                    CellValue::MV,
                );
            }
        }
        CellValue::BOXTIMES

    } else if is_box_leak_call(func_call_details) {
        // Box::leak consumes the Box and intentionally relinquishes automatic
        // destruction.  The returned &mut T is the surviving handle to the
        // same allocation.  At allocation level CREMA records this as MV
        // ("ownership no longer automatically reclaimed").
        let full_ret = full_local_name(return_place);
        if let Some(source) = first_call_local_from_details(func_call_details) {
            replace_consumed_local_with(
                &mut new_mem,
                &source,
                full_ret,
                CellValue::MV,
            );
        } else {
            new_mem
                .state
                .insert(Allocation::new(full_ret), CellValue::MV);
        }
        CellValue::MV

    } else if is_vec_from_raw_parts_call(func_call_details)
        || is_string_from_raw_parts_call(func_call_details)
    {
        // from_raw_parts transfers responsibility for an existing raw
        // allocation to an owning container.  The current implementation stores
        // one CellValue per alias component, so we keep the component in MV
        // while adding the returned owner as an alias; normal Drop accounting
        // will still reclaim the component.
        let full_ret = full_local_name(return_place);
        if let Some(source) = first_call_local_from_details(func_call_details) {
            if let Some(old_alloc) = new_mem.get_allocation(&source) {
                new_mem.state.remove(&old_alloc);
                let mut set = old_alloc.set;
                set.insert(full_ret.clone());
                new_mem
                    .state
                    .insert(Allocation { set }, CellValue::MV);
            } else {
                new_mem
                    .state
                    .insert(Allocation::new(full_ret.clone()), CellValue::MV);
            }
        } else {
            new_mem
                .state
                .insert(Allocation::new(full_ret.clone()), CellValue::MV);
        }
        CellValue::MV

    } else if is_borrowed_raw_pointer_view_call(func_call_details) {
        // as_ptr/as_mut_ptr-style APIs expose a borrowed raw pointer; they do
        // not consume the owner and do not transfer deallocation responsibility.
        let full_ret = full_local_name(return_place);
        if let Some(source) = first_call_local_from_details(func_call_details) {
            let value = new_mem.get_cell_value(&source);
            if value != CellValue::BOTTOM && new_mem.get_allocation(&source).is_some() {
                new_mem.propagate_cell_value(&source, &full_ret);
                value
            } else {
                new_mem.assign_local_value(&full_ret, CellValue::TOP);
                CellValue::TOP
            }
        } else {
            new_mem.assign_local_value(&full_ret, CellValue::TOP);
            CellValue::TOP
        }

    } else if memory_events::is_into_vec_transfer_call(func_call_details) {
        // Box<[T]>::into_vec / slice into_vec transfers ownership of the same
        // backing allocation.  It is NOT a fresh allocation site.  Preserve
        // the allocation component while replacing the consumed owner local by
        // the returned Vec owner.
        let full_ret = full_local_name(return_place);
        if let Some(source) = first_call_local_from_details(func_call_details) {
            let value = new_mem.get_cell_value(&source);
            if new_mem.get_allocation(&source).is_some() {
                replace_consumed_local_with(
                    &mut new_mem,
                    &source,
                    full_ret,
                    if value == CellValue::BOTTOM { CellValue::TOP } else { value },
                );
                if value == CellValue::BOTTOM { CellValue::TOP } else { value }
            } else {
                new_mem.assign_local_value(&full_ret, CellValue::TOP);
                CellValue::TOP
            }
        } else {
            new_mem.assign_local_value(&full_ret, CellValue::TOP);
            CellValue::TOP
        }

    } else if memory_events::is_exchange_malloc_call(func_call_details) {
        // exchange_malloc is the infallible allocation primitive used by Box/
        // Vec lowering on the pinned toolchain: on its normal return the
        // allocation exists (allocation failure does not return normally).
        let full_ret = full_local_name(return_place);
        new_mem.assign_local_value(&full_ret, CellValue::ALLOC);
        CellValue::ALLOC

    } else if is_raw_alloc_call(func_call_details)
        || is_raw_alloc_zeroed_call(func_call_details)
    {
        // std::alloc::{alloc,alloc_zeroed} return a nullable raw pointer.
        // Without a NULL lattice element, TOP is the sound abstraction:
        // either no allocation was returned, or a live raw allocation exists.
        let full_ret = full_local_name(return_place);
        if let Some(old) = new_mem.get_allocation(&full_ret) {
            new_mem.state.remove(&old);
        }
        new_mem
            .state
            .insert(Allocation::new(full_ret), CellValue::TOP);
        CellValue::TOP

    } else if is_raw_dealloc_call(func_call_details) {
        // dealloc invalidates the allocation but returns ().
        if let Some(source) = first_call_local_from_details(func_call_details) {
            if new_mem.get_allocation(&source).is_some() {
                new_mem = update_state(new_mem, &source, CellValue::FREED);
            }
        }
        CellValue::BOXTIMES

    } else if is_c_free_call_text(func_call_details, &ffi_functions) {
        // A Rust call site can still invoke the C malloc-family deallocator.
        //
        // Keep the primary abstract state conservative: a C-origin pointer is
        // nullable and CellValue has no NULL element/path split.  The detector
        // records the explicit free event after complete alias/provenance
        // discovery.
        CellValue::BOXTIMES

    } else if is_ptr_read_call(func_call_details) {
        // ptr::read leaves the source memory unchanged.  The returned T can be
        // arbitrary (and for non-Copy T can duplicate ownership), which this
        // local-based memory domain cannot classify without type/drop metadata.
        // Keep the memory unchanged and conservatively classify only the return.
        CellValue::TOP

    } else if is_ptr_write_call(func_call_details)
        || is_ptr_drop_in_place_call(func_call_details)
    {
        // Both APIs require a valid destination pointer.  Their nested
        // destructor/resource effect is type-dependent and is handled only as
        // a deferred "use" in the detector for now.
        CellValue::BOXTIMES

    } else if is_raw_realloc_call(func_call_details) {
        // Rust `GlobalAlloc::realloc` is branch-sensitive:
        //
        // * non-null result: ownership of the old block has been transferred
        //   and *any* access through the old pointer is UB, even when the
        //   allocation stayed in place;
        // * null result: ownership was not transferred and the old allocation
        //   is unchanged.
        //
        // CellValue has neither a NULL element nor a disjunction capable of
        // representing {old-live-on-failure, old-invalid-on-success}.  Keeping
        // the old component at MV/ALLOC would therefore be an under-
        // approximation.  The sound non-disjunctive abstraction is TOP for
        // the old alias component and TOP for the nullable returned pointer.
        // Allocation identity separately records the MAY old-or-fresh relation.
        if let Some(source) = first_call_local_from_details(func_call_details) {
            if new_mem.get_allocation(&source).is_some() {
                new_mem.set_cell_value(&source, CellValue::TOP);
            }
        }
        let full_ret = full_local_name(return_place);
        new_mem.assign_local_value(&full_ret, CellValue::TOP);
        CellValue::TOP

    } else if

    // REGEX
    BOX_NEW_REGEX.is_match(func_call_details) 
    || BOX_VEC_NEW_REGEX.is_match(func_call_details) 
    || VEC_ALLOC_REGEX.is_match(func_call_details) 
    ||  BOX_NEW_NODE_GENERIC_REGEX.is_match(func_call_details)
    // -
    || func_call_details.contains("std::boxed::Box::<i32>::new")  || func_call_details.contains("std::boxed::Box::<u32>::new")
    // -
    || func_call_details.contains("std::boxed::Box::<f32>::new")
    || func_call_details.contains("std::boxed::Box::<f64>::new")
    || func_call_details.contains("std::boxed::Box::<&str>::new")
    || func_call_details.contains("std::boxed::Box::<u8>::new")
    || func_call_details.contains("std::boxed::Box::<i8>::new")
    || func_call_details.contains("std::boxed::Box::<i16>::new")
    || func_call_details.contains("std::boxed::Box::<u16>::new")
    || func_call_details.contains("std::boxed::Box::<i64>::new")
    || func_call_details.contains("std::boxed::Box::<u64>::new")
    || func_call_details.contains("std::boxed::Box::<i128>::new")
    || func_call_details.contains("std::boxed::Box::<u128>::new")
    || func_call_details.contains("std::boxed::Box::<bool>::new")
    || func_call_details.contains("std::boxed::Box::<char>::new")
    || func_call_details.contains("std::boxed::Box::<usize>::new") || func_call_details.contains("std::boxed::Box::<isize>::new")
    // --
    || func_call_details.contains("std::boxed::Box::<std::vec::Vec<char>>::new")
    // --
    || func_call_details.contains("std::boxed::Box::<std::string::String>::new") 
    || func_call_details.contains("<std::ffi::CString as std::convert::From<&std::ffi::CStr>>::from") || func_call_details.contains("std::ffi::CString::new::")
    // - 
    //ALLOC option<F>
    || func_call_details.contains("std::boxed::Box::<std::option::Option<F>>::new")
    // -
    // ALLOC MSG SENDER
    || func_call_details.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::new")

    {
    // DO
        // Box::new allocates on the heap for the return_place
        // different heap allocations via Box::new will yield distinct allocation sets, 
        // so that when have pointer copy operations will correctly join the destination into the source’s allocation set
        // remove any existing allocation for the return_place
        let ret_full = full_local_name(return_place);
        if let Some(alloc) = new_mem.get_allocation(&ret_full) {
            new_mem.state.remove(&alloc);
        }
        // create a fresh allocation for the return_place
        let new_alloc = Allocation::new(ret_full.clone());
        new_mem.state.insert(new_alloc, CellValue::ALLOC);
        CellValue::ALLOC
  
    } else if func_call_details.contains("JEMALLOC, MIMALLOC IF SPEC BY USER ANOTHER ALLOC FUNCTION CASE, need to check on 1.86 ") {
        // For into_vec, update the abstract state based on the argument.
        if let Some(arg_val) = extract_arg_from_details(func_call_details) {
            new_mem.set_cell_value(&full_local_name(&arg_val), CellValue::ALLOC);
        }
        CellValue::ALLOC    
        
    // MV (forget the ownership) (form ALLOC can go to MV)
    } else if func_call_details.contains("std::boxed::Box::<i32>::into_raw") || func_call_details.contains("std::boxed::Box::<u32>::into_raw")
    || func_call_details.contains("std::boxed::Box::<u8>::into_raw") || func_call_details.contains("std::boxed::Box::<i8>::into_raw")
    || func_call_details.contains("std::boxed::Box::<i16>::into_raw") || func_call_details.contains("std::boxed::Box::<u16>::into_raw")
    || func_call_details.contains("std::boxed::Box::<i64>::into_raw") || func_call_details.contains("std::boxed::Box::<u64>::into_raw")
    || func_call_details.contains("std::boxed::Box::<i128>::into_raw") || func_call_details.contains("std::boxed::Box::<u128>::into_raw")
    || func_call_details.contains("std::boxed::Box::<&str>::into_raw")
    || func_call_details.contains("std::boxed::Box::<f64>::into_raw") || func_call_details.contains("std::boxed::Box::<f32>::into_raw")
    || func_call_details.contains("std::boxed::Box::<bool>::into_raw") || func_call_details.contains("std::boxed::Box::<char>::into_raw")
    || func_call_details.contains("std::boxed::Box::<usize>::into_raw") || func_call_details.contains("std::boxed::Box::<isize>::into_raw")
    
    || func_call_details.contains("std::boxed::Box::<std::string::String>::into_raw")
    || func_call_details.contains("std::ffi::CString::into_raw")

    // into raw regex
    || VEC_INTO_RAW_REGEX.is_match(func_call_details)
    || BOX_INTO_RAW_NODE_GENERIC_REGEX.is_match(func_call_details)

    //|| func_call_details.contains("std::boxed::Box::<Node<u32>>::into_raw")

    //vec
    || func_call_details.contains("std::boxed::Box::<std::vec::Vec<char>>::into_raw")
    // MV option<F>
    || func_call_details.contains("std::boxed::Box::<std::option::Option<F>>::into_raw")
    // MV MSG SENDER
    || func_call_details.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::into_raw")

    {
    // DO
        let moved_var = extract_moved_var(func_call_details);
        let full_moved = full_local_name(&moved_var);
        let full_ret = full_local_name(return_place);
        
        // Rimuovi l'allocazione esistente della variabile spostata (es. _5)
        if let Some(old_alloc) = new_mem.get_allocation(&full_moved) {
            new_mem.state.remove(&old_alloc);
        }
        new_mem.set_cell_value(&full_moved, CellValue::BOTTOM);
        
        // Crea una NUOVA allocazione per il return_place (_8)
        if let Some(alloc) = new_mem.get_allocation(&full_ret) {
            new_mem.state.remove(&alloc);
        }
        let new_alloc = Allocation::new(full_ret.clone());
        new_mem.state.insert(new_alloc, CellValue::MV);
        
        CellValue::MV
    
    // FROM RAW (get back the ownership)
    } else if func_call_details.contains("std::boxed::Box::<i32>::from_raw") || func_call_details.contains("std::boxed::Box::<u32>::from_raw")
    || func_call_details.contains("std::boxed::Box::<u8>::from_raw") || func_call_details.contains("std::boxed::Box::<i8>::from_raw")
    || func_call_details.contains("std::boxed::Box::<i16>::from_raw") || func_call_details.contains("std::boxed::Box::<u16>::from_raw")
    || func_call_details.contains("std::boxed::Box::<i64>::from_raw") || func_call_details.contains("std::boxed::Box::<u64>::from_raw")
    || func_call_details.contains("std::boxed::Box::<i128>::from_raw") || func_call_details.contains("std::boxed::Box::<u128>::from_raw")
    || func_call_details.contains("std::boxed::Box::<f32>::from_raw") || func_call_details.contains("std::boxed::Box::<f64>::from_raw")
    || func_call_details.contains("std::boxed::Box::<bool>::from_raw") || func_call_details.contains("std::boxed::Box::<char>::from_raw")
    || func_call_details.contains("std::boxed::Box::<usize>::from_raw") || func_call_details.contains("std::boxed::Box::<isize>::from_raw")

    || func_call_details.contains("std::boxed::Box::<&str>::from_raw")
    || func_call_details.contains("std::boxed::Box::<std::string::String>::from_raw")
    || func_call_details.contains("std::boxed::Box::<std::ffi::CStr>::from_raw")
    || func_call_details.contains("std::ffi::CStr::from_raw")
    || func_call_details.contains("std::boxed::Box::<std::ffi::CString>::from_raw")
    || func_call_details.contains("std::ffi::CString::from_raw") 
    // from raw regex
    ||  FROM_RAW_REGEX.is_match(func_call_details) 
    || BOX_VEC_FROM_RAW_REGEX.is_match(func_call_details)
    || VEC_FROM_RAW_REGEX.is_match(func_call_details)
    || BOX_FROM_RAW_NODE_GENERIC_REGEX.is_match(func_call_details)
    // vec
    || func_call_details.contains("std::boxed::Box::<std::vec::Vec<char>>::from_raw")
    // from raw msg sender
    || func_call_details.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::from_raw")


// THIS VERSION: union the return variable into the existing allocation set. 
        {
            // get MIR name: eg. "_2"
            let moved_var = extract_moved_var(func_call_details);
            let full_moved = full_local_name(&moved_var);
            // get return var : eg. "_3" -> "Local(_3)"
            let full_ret   = full_local_name(return_place);
    
            if let Some(old_alloc) = new_mem.get_allocation(&full_moved) {
                // construct a new allocation with the old set, removing the moved variable and adding the return variable
                let mut new_set = old_alloc.set.clone();
                new_set.insert(full_ret.clone());
                let new_alloc = Allocation { set: new_set };

                // update old allocation with the new one
                new_mem.state.remove(&old_alloc);
                new_mem.state.insert(new_alloc, CellValue::MV);
            } else {
                // fallback: if the allocation was not found, create a fresh one
                let new_alloc = Allocation::new(full_ret.clone());
                new_mem.state.insert(new_alloc, CellValue::MV);
            }
            CellValue::MV


    // EXPLICIT std::mem::drop
    } else if is_explicit_mem_drop(func_call_details) {
        // Dropping a raw pointer is a no-op with respect to the pointee:
        // *mut T / *const T do not own the allocation.
        if is_raw_pointer_mem_drop(func_call_details) {
            // The pointee is untouched; the function result is `()`, a
            // defined non-heap value represented by BOXTIMES.
            CellValue::BOXTIMES
        } else {
            // For an owning value, free ONLY the tracked allocation that
            // contains the dropped argument. Never mark the entire abstract
            // memory as FREED.
            if let Some(full_dropped) = explicit_drop_arg_from_details(func_call_details) {
                if new_mem.get_allocation(&full_dropped).is_some() {
                    new_mem = update_state(new_mem, &full_dropped, CellValue::FREED);
                }
            }

            // std::mem::drop returns `()`: no heap allocation is produced,
            // but the return local contains a defined non-heap/unit value.
            CellValue::BOXTIMES
        }
    } else {
        // default: do nothing
        let full_ret = full_local_name(return_place);
        new_mem.get_cell_value(&full_ret)
        //CellValue::BOTTOM
    };
    (ret_val, new_mem)
}




/// v6O telemetry-only classifier for the existing MIR statement transfer.
///
/// This function does not change the abstract semantics.  It mirrors the
/// positive dispatch in `apply_mir_statement`/`eval_rvalue` so the coverage
/// report can distinguish dedicated transfer rules from deliberate TOP and
/// from the sparse fallback.  The returned vocabulary is intentionally small
/// and stable for artifact comparison.
pub fn classify_mir_statement_coverage(stmt: &MirStatement) -> &'static str {
    match stmt.kind.as_str() {
        "Nop" => "precise",
        "Assign" => {
            let Some(rvalue) = stmt.rvalue.as_deref() else {
                return "unmodeled";
            };
            let trimmed = rvalue.trim();

            // Historical `&(*...)` branch deliberately preserves state and is
            // therefore not counted as a positively modeled transfer.
            if trimmed.contains("&(*") {
                return "unmodeled";
            }

            if direct_stack_borrow_source(trimmed).is_some()
                || trimmed.starts_with('&')
                || pointer_cast_source(trimmed).is_some()
                || pointer_offset_source(trimmed).is_some()
                || is_closure_aggregate_rvalue(trimmed)
                || is_copy_for_deref_rvalue(trimmed)
                || is_direct_deref_use_rvalue(trimmed)
            {
                return "conservative";
            }

            if exact_local_operand(trimmed, "copy ").is_some()
                || exact_local_operand(trimmed, "move ").is_some()
                || is_scalar_const_rvalue(trimmed)
                || is_scalar_nullary_rvalue(trimmed)
                || scalar_cast_target(trimmed).is_some()
                || comparison_rvalue_args(trimmed).is_some()
                || binary_rvalue_args(trimmed).is_some()
                || unary_rvalue_arg(trimmed).is_some()
            {
                return "precise";
            }

            "unmodeled"
        }
        // Current memory transfer leaves all other MIR statement kinds
        // unchanged.  Telemetry records these as explicit coverage gaps rather
        // than silently calling the no-op precise.
        _ => "unmodeled",
    }
}

/// Lower-bound inventory of external Rust calls with an explicit memory effect
/// in the current CREMA transfer.  This is telemetry only: returning false does
/// not imply that a call is semantically opaque in every CREMA component.
/// Keeping this helper next to the actual transfer predicates avoids a second
/// text-pattern vocabulary in the coverage exporter.
pub fn has_explicit_rust_call_summary(s: &str) -> bool {
    is_box_new_call(s)
        || is_owning_into_raw_call(s)
        || is_owning_from_raw_call(s)
        || is_mem_forget_call(s)
        || is_box_leak_call(s)
        || is_vec_from_raw_parts_call(s)
        || is_string_from_raw_parts_call(s)
        || is_raw_alloc_call(s)
        || is_raw_alloc_zeroed_call(s)
        || is_raw_dealloc_call(s)
        || is_raw_realloc_call(s)
        || is_cstr_from_ptr_call(s)
        || is_borrowed_raw_pointer_view_call(s)
        || is_pointer_memory_use_call(s)
        || memory_events::is_into_vec_transfer_call(s)
        || memory_events::is_exchange_malloc_call(s)
        || is_explicit_mem_drop(s)
        || s.contains("std::result::Result::<std::ffi::CString, std::ffi::NulError>::expect")
}

pub fn apply_mir_statement(mem: &AbstractMemory, taint: &mut TaintStateMap, stmt: &MirStatement) -> AbstractMemory {
    let mut new_mem = mem.clone();
    match stmt.kind.as_str() {
        "Nop" => { /* no change */ }
        "Assign" => {
            if let Some(rvalue) = &stmt.rvalue {
                // A direct MIR Ref/AddressOf points to the stack *place*.
                // It must not be merged with an allocation owned/denoted by the
                // value stored in that place.
                if direct_stack_borrow_source(rvalue).is_some() {
                    if let Some(dest) = &stmt.place {
                        let dest_key = full_local_name(dest);
                        new_mem.assign_local_value(&dest_key, CellValue::TOP);
                        taint.remove(&dest_key);
                    }
                    return new_mem;
                }

                if rvalue.starts_with('&') {
                    if let Some(dest) = &stmt.place {
                        let dest_key = full_local_name(dest);

                        // Remaining forms include dereference/projection borrows,
                        // which can denote an already tracked pointee allocation.
                        if let Some(src_var) = extract_first_local_token(rvalue) {
                            let src_key = full_local_name(&src_var);
                            let src_value = new_mem.get_cell_value(&src_key);

                            if src_value != CellValue::BOTTOM
                                && new_mem.get_allocation(&src_key).is_some()
                            {
                                new_mem.propagate_cell_value(&src_key, &dest_key);

                                let t_src = taint.get(&src_key).cloned().unwrap_or_default();
                                let t_dest = taint.get(&dest_key).cloned().unwrap_or_default();
                                let joined =
                                    t_src.union(&t_dest).cloned().collect::<HashSet<_>>();
                                taint.insert(src_key.clone(), joined.clone());
                                taint.insert(dest_key, joined);
                            } else {
                                new_mem.assign_local_value(&dest_key, CellValue::TOP);
                                taint.remove(&dest_key);
                            }
                        } else {
                            new_mem.assign_local_value(&dest_key, CellValue::TOP);
                            taint.remove(&dest_key);
                        }
                    }
                    return new_mem;
                }

                if rvalue.contains("Box::new") || rvalue.contains("vec::new") {
                    if let Some(_var) = &stmt.place {
                        // update taint for alloc
                    }
                } else if rvalue.contains("move")
                    && !rvalue.contains("[")
                    && pointer_cast_source(rvalue).is_none()
                { // only plain move transfers ownership; pointer casts preserve allocation identity
                    // MOVE semantics: TRANSFER the OWNERSHIP of the value
                    let src_var = extract_moved_var(rvalue);
                    if !src_var.is_empty() {
                        if let Some(dest) = &stmt.place {
                            let src_key = full_local_name(&src_var);
                            let dest_key = full_local_name(dest);
                            // propagate the cell value from source to destination.
                            new_mem.propagate_cell_value(&src_key, &dest_key);
                            let propagated_val = new_mem.get_cell_value(&dest_key);
                            // after moving, set the source cell to BOTTOM.
                            new_mem.set_cell_value(&src_key, CellValue::BOTTOM);
                            // reassign the destination cell
                            new_mem.set_cell_value(&dest_key, propagated_val);
                            // UPDATE TAINT
                            // retreive the taint state of the src_key
                            let taint_state_src = taint.get(&src_key).cloned().unwrap_or_default();
                            // remove the taint state of the dest_key
                            taint.remove(&dest_key);
                            // update the taint state of the dest_key with the taint state of the src_key
                            taint.insert(dest_key.clone(), taint_state_src);
                            //REMOVE the taint state of the src_key
                            taint.remove(&src_key);
                        }
                    }
                } else if let Some(src_var) = pointer_cast_source(rvalue) {
                    let src_key = full_local_name(&src_var);
                    let dest_key = full_local_name(
                        stmt.place
                            .as_ref()
                            .expect("pointer cast assignment must have destination"),
                    );
                    let value = new_mem.get_cell_value(&src_key);

                    if is_heap_dependent_value(value)
                        && new_mem.get_allocation(&src_key).is_some()
                    {
                        new_mem.propagate_cell_value(&src_key, &dest_key);
                        let tags = taint.get(&src_key).cloned().unwrap_or_default();
                        taint.entry(dest_key).or_default().extend(tags);
                    } else {
                        // Integer/scalar -> pointer or untracked provenance:
                        // the resulting raw pointer may point anywhere.
                        new_mem.assign_local_value(&dest_key, CellValue::TOP);
                        taint.remove(&dest_key);
                    }

                } else if let Some(src_var) = pointer_offset_source(rvalue) {
                    let src_key = full_local_name(&src_var);
                    let dest_key = full_local_name(
                        stmt.place
                            .as_ref()
                            .expect("Offset assignment must have destination"),
                    );
                    let value = new_mem.get_cell_value(&src_key);

                    if is_heap_dependent_value(value)
                        && new_mem.get_allocation(&src_key).is_some()
                    {
                        new_mem.propagate_cell_value(&src_key, &dest_key);
                        let tags = taint.get(&src_key).cloned().unwrap_or_default();
                        taint.entry(dest_key).or_default().extend(tags);
                    } else {
                        new_mem.assign_local_value(&dest_key, CellValue::TOP);
                        taint.remove(&dest_key);
                    }

                }                 else if rvalue.contains("&(*") {
                    println!();
                    // copy semantics: propagate the cell value from the place in the rvalue expression to the local destination
                    /* 
                    if let Some(rvalue_var) = extract_var_from_rvalue(rvalue) {
                        if let Some(dest) = &stmt.place {
                            let src_key = full_local_name(&rvalue_var);
                            let dest_key = full_local_name(dest);
                            // For a copy, propagate without modifying the source.
                            new_mem.propagate_cell_value(&src_key, &dest_key);
                            
                            taint.entry(full_local_name(dest))
                                .or_insert_with(HashSet::new)
                                .insert("use".to_string());
                        }
                    } */
                } else if let Some(var) = &stmt.place {
                    // Evaluate the actual MIR Rvalue, not the enclosing
                    // StatementKind::Assign Debug string.
                    let dest_key = full_local_name(var);
                    let v = eval_rvalue(rvalue, mem);

                    // Direct copies of tracked heap-dependent values preserve
                    // may-alias information. Scalar copies do not create alias
                    // components merely because they share BOXTIMES.
                    if let Some(src_var) = exact_local_operand(rvalue, "copy ") {
                        let src_key = full_local_name(&src_var);

                        if matches!(
                            v,
                            CellValue::ALLOC
                                | CellValue::FREED
                                | CellValue::MB
                                | CellValue::IMMB
                                | CellValue::MV
                                | CellValue::TOP
                        ) && mem.get_allocation(&src_key).is_some()
                        {
                            new_mem.propagate_cell_value(&src_key, &dest_key);

                            let t_src = taint.get(&src_key).cloned().unwrap_or_default();
                            taint.entry(dest_key)
                                .or_insert_with(HashSet::new)
                                .extend(t_src);
                        } else {
                            new_mem.assign_local_value(&dest_key, v);
                            taint.remove(&dest_key);
                        }
                    } else {
                        // Ordinary assignment overwrites only the destination
                        // local. It must detach that local from any old alias
                        // component rather than changing all of its old aliases.
                        new_mem.assign_local_value(&dest_key, v);

                        if matches!(v, CellValue::BOXTIMES | CellValue::BOTTOM) {
                            taint.remove(&dest_key);
                        }
                    }
                }
            }
        }
        _ => { /* nisba, oth kinds remain unchanged */ }
    }
    new_mem
}



pub fn apply_mir_terminator(mem: &AbstractMemory,taint: &mut TaintStateMap,term: &MirTerminator) -> AbstractMemory {let mut new_mem = mem.clone();

    match term {
        MirTerminator::Call {details, function_called, arguments, return_place, allocation_disposition_evidence, ..} => {
            // CStr::from_ptr creates a borrowed C-string view. It does not
            // retake ownership of the pointed allocation.
            if is_cstr_from_ptr_call(function_called) {
                if let Some(first_arg) = arguments.get(0) {
                    let src_var = extract_arg_name(&first_arg.arg);
                    let full_src = full_local_name(&src_var);
                    let full_ret = full_local_name(return_place);
                    let value = new_mem.get_cell_value(&full_src);

                    if value != CellValue::BOTTOM
                        && new_mem.get_allocation(&full_src).is_some()
                    {
                        new_mem.propagate_cell_value(&full_src, &full_ret);

                        let tags = taint.get(&full_src).cloned().unwrap_or_default();
                        taint.entry(full_ret).or_default().extend(tags);
                    } else {
                        new_mem.assign_local_value(&full_ret, CellValue::TOP);
                        taint.remove(&full_ret);
                    }
                }

            // HANDLE Result::expect on CString
            } else if function_called.contains("std::result::Result::<std::ffi::CString, std::ffi::NulError>::expect") 
           /* 
           con CStr(args).to_string_lossy() ritorna un valore di tipo Cow<'_, str> (copy on write)
           */
           // || function_called.contains("alloc::ffi::c_str::<impl std::ffi::CStr>::to_string_lossy") 
        
            {
                if let Some(first_arg) = arguments.get(0) {
                    // extract source and return variable names
                    let src_var = extract_arg_name(&first_arg.arg);
                    let full_src = full_local_name(&src_var);
                    let full_ret = full_local_name(return_place);

                    // clone taint tags to avoid simultaneous borrows
                    let tags_to_propagate = taint
                        .get(&full_src)
                        .map(|set| set.iter().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();

                    // propagate cell value
                    let val = new_mem.get_cell_value(&full_src);

                    // propagate allocation: move ownership from src to ret
                    if let Some(old_alloc) = new_mem.get_allocation(&full_src) {
                        // build new allocation set: replace src with ret
                        let mut new_set = old_alloc.set.clone();
                        new_set.remove(&full_src);
                        new_set.insert(full_ret.clone());
                        let new_alloc = Allocation { set: new_set };
                        // update memory
                        new_mem.state.remove(&old_alloc);
                        new_mem.state.insert(new_alloc, val);
                    } else {
                        // fallback: set via update_state
                        new_mem = update_state(new_mem, &full_ret, val);
                    }
                    // propagate taint tags
                    let entry = taint.entry(full_ret.clone())
                        .or_insert_with(HashSet::new);
                    for tag in tags_to_propagate {
                        entry.insert(tag);
                    }
                    // clear source cell, allocation and taint
                    new_mem.set_cell_value(&full_src, CellValue::BOTTOM);
                    if let Some(old_alloc) = new_mem.get_allocation(&full_src) {
                        new_mem.state.remove(&old_alloc);
                    }
                    taint.remove(&full_src);
                }


            } else {
                // default MIR call handling
                let certified_raw_pointer_drop = allocation_disposition_evidence
                    .as_ref()
                    .is_some_and(|e| matches!(
                        e.kind,
                        RustAllocationDispositionEvidenceKind::MemDropRawPointer
                    ));
                let explicit_drop = is_explicit_mem_drop(function_called)
                    || certified_raw_pointer_drop;
                let raw_pointer_drop = is_raw_pointer_mem_drop(function_called)
                    || certified_raw_pointer_drop;

                // A producer-certified raw-pointer drop is a pointee no-op even
                // if pretty MIR spelling changes.  This guard is semantically
                // equivalent to the historical textual branch on the frozen
                // corpus, but prevents a future formatter change from turning
                // `drop(*mut T)` into a heap deallocation.
                let (ret_value, updated_mem) = if certified_raw_pointer_drop {
                    (CellValue::BOXTIMES, new_mem.clone())
                } else {
                    transfer_call(&new_mem, details, return_place)
                };
                new_mem = updated_mem;
                let full_ret_place = full_local_name(return_place);

                // transfer_call applies the heap effect of std::mem::drop
                // to the argument. The MIR destination still receives `()`,
                // represented by BOXTIMES. Overwrite/detach that local without
                // manufacturing a heap alias.
                if explicit_drop {
                    new_mem.assign_local_value(&full_ret_place, CellValue::BOXTIMES);
                } else {
                    new_mem = update_state(new_mem, &full_ret_place, ret_value);
                }

                // UPDATE TAINT
                if is_box_new_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    taint.remove(&full_ret);

                } else if is_owning_into_raw_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    let mut tags = HashSet::new();
                    tags.insert("assign".to_string());
                    if let Some(source) = first_call_local_from_details(details) {
                        if let Some(old) = taint.remove(&source) {
                            tags.extend(old);
                        }
                    }
                    taint.insert(full_ret, tags);

                } else if is_owning_from_raw_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    let mut tags = HashSet::new();
                    tags.insert("assign".to_string());
                    if let Some(source) = first_call_local_from_details(details) {
                        if let Some(old) = taint.get(&source) {
                            tags.extend(old.iter().cloned());
                        }
                    }
                    taint.entry(full_ret).or_default().extend(tags);

                } else if is_mem_forget_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    taint.remove(&full_ret);

                    if let Some(source) = first_call_local_from_details(details) {
                        let ghost = leak_ghost_name(&source);
                        taint.remove(&source);
                        taint
                            .entry(ghost)
                            .or_default()
                            .insert("assign".to_string());
                    }

                } else if is_box_leak_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    taint
                        .entry(full_ret)
                        .or_default()
                        .insert("assign".to_string());

                    if let Some(source) = first_call_local_from_details(details) {
                        taint.remove(&source);
                    }

                } else if is_borrowed_raw_pointer_view_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    if let Some(source) = first_call_local_from_details(details) {
                        let tags = taint.get(&source).cloned().unwrap_or_default();
                        if !tags.is_empty() {
                            taint.entry(full_ret).or_default().extend(tags);
                        }
                    }

                } else if is_vec_from_raw_parts_call(function_called)
                    || is_string_from_raw_parts_call(function_called)
                {
                    let full_ret = full_local_name(return_place);
                    let mut tags = HashSet::new();
                    tags.insert(TAINT_ASSIGN.to_string());

                    if let Some(source) = first_call_local_from_details(details) {
                        if let Some(source_tags) = taint.get(&source) {
                            tags.extend(source_tags.iter().cloned());
                        }
                    }

                    taint.entry(full_ret).or_default().extend(tags);

                } else if memory_events::is_into_vec_transfer_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    let mut tags = HashSet::new();
                    tags.insert(TAINT_ASSIGN.to_string());
                    if let Some(source) = first_call_local_from_details(details) {
                        if let Some(source_tags) = taint.remove(&source) {
                            tags.extend(source_tags);
                        }
                    }
                    taint.insert(full_ret, tags);

                } else if memory_events::is_exchange_malloc_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    taint
                        .entry(full_ret)
                        .or_default()
                        .insert(TAINT_ASSIGN.to_string());

                } else if is_raw_alloc_call(function_called)
                    || is_raw_alloc_zeroed_call(function_called)
                {
                    let full_ret = full_local_name(return_place);
                    taint
                        .entry(full_ret)
                        .or_default()
                        .insert(TAINT_ASSIGN.to_string());

                } else if is_raw_dealloc_call(function_called) {
                    let full_ret = full_local_name(return_place);
                    taint.remove(&full_ret);

                    if let Some(source) = first_call_local_from_details(details) {
                        taint
                            .entry(source)
                            .or_default()
                            .insert("free".to_string());
                    }

                } else if is_ptr_read_call(function_called)
                    || is_ptr_write_call(function_called)
                    || is_ptr_drop_in_place_call(function_called)
                    || is_raw_realloc_call(function_called)
                {
                    // State/provenance is handled by the detector and/or the
                    // conservative return value above. Do not manufacture
                    // ownership-transfer taint here.

                } else if function_called.contains("NOT_HANDLED") {
                    taint.entry(full_ret_place.clone())
                        .or_insert_with(HashSet::new)
                        .insert("NOT_HANDLED".to_string());
                    // TAINT FOR ASSING (ABSTRACT: MV => TAINT:ASSIGN)
                } else if function_called.contains("std::boxed::Box::<i32>::into_raw") || function_called.contains("std::boxed::Box::<u32>::into_raw")
                    || function_called.contains("std::boxed::Box::<f64>::into_raw")
                    || function_called.contains("std::boxed::Box::<u8>::into_raw")  || function_called.contains("std::boxed::Box::<i8>::into_raw")
                    || function_called.contains("std::boxed::Box::<u16>::into_raw")  || function_called.contains("std::boxed::Box::<i16>::into_raw")
                    || function_called.contains("std::boxed::Box::<i64>::into_raw")  || function_called.contains("std::boxed::Box::<u64>::into_raw")
                    || function_called.contains("std::boxed::Box::<i128>::into_raw")  || function_called.contains("std::boxed::Box::<u128>::into_raw")
                    || function_called.contains("std::boxed::Box::<&str>::into_raw")
                    || function_called.contains("std::boxed::Box::<std::string::String>::into_raw")
                    || function_called.contains("std::ffi::CString::into_raw")
                    || function_called.contains("std::boxed::Box::<bool>::into_raw") || function_called.contains("std::boxed::Box::<char>::into_raw")
                    || function_called.contains("std::boxed::Box::<usize>::into_raw") || function_called.contains("std::boxed::Box::<isize>::into_raw")
                    // INTO RAW regex
                    || VEC_INTO_RAW_REGEX.is_match(&function_called)
                    || BOX_INTO_RAW_NODE_GENERIC_REGEX.is_match(&function_called)

                    //vec
                    || function_called.contains("std::boxed::Box::<std::vec::Vec<char>>::into_raw")
                    // MV option<F>
                    || function_called.contains("std::boxed::Box::<std::option::Option<F>>::into_raw")
                    // MV MSG SENDER
                    || function_called.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::into_raw")
                    

                {
                    taint.entry(full_ret_place.clone())
                        .or_insert_with(HashSet::new)
                        .insert("assign".to_string());
                    let moved_var = extract_moved_var(details);
                    if !moved_var.is_empty() {
                        let moved_key = full_local_name(&moved_var);
                        taint.remove(&moved_key);
                    }
                } else if function_called.contains("std::boxed::Box::<i32>::new")
                    || function_called.contains("vec::new")
                    || function_called.contains("<std::ffi::CString as std::convert::From<&std::ffi::CStr>>::from")
                    || function_called.contains("std::ffi::CString::new::")
                {
                    // Allocation-only calls: state already set via update_state
                } else if function_called.contains("std::boxed::Box::<i32>::from_raw") || function_called.contains("std::boxed::Box::<u32>::from_raw")
                    || function_called.contains("std::boxed::Box::<f64>::from_raw") || function_called.contains("std::boxed::Box::<f32>::from_raw")
                    || function_called.contains("std::boxed::Box<u8>::from_raw") || function_called.contains("std::boxed::Box::<i8>::from_raw")
                    || function_called.contains("std::boxed::Box::<i16>::from_raw") || function_called.contains("std::boxed::Box::<u16>::from_raw")
                    || function_called.contains("std::boxed::Box::<i64>::from_raw") || function_called.contains("std::boxed::Box::<u64>::from_raw")
                    || function_called.contains("std::boxed::Box::<i128>::from_raw") || function_called.contains("std::boxed::Box::<u128>::from_raw")
                    || function_called.contains("std::boxed::Box::<&str>::from_raw")
                    || function_called.contains("std::boxed::Box::<std::string::String>::from_raw")
                    || function_called.contains("std::ffi::CString::from_raw")
                    || function_called.contains("std::boxed::Box::<bool>::from_raw") || function_called.contains("std::boxed::Box::<char>::from_raw")
                    || function_called.contains("std::boxed::Box::<usize>::from_raw") || function_called.contains("std::boxed::Box::<isize>::from_raw")
                    // FROM RAW regex
                    ||  FROM_RAW_REGEX.is_match(&function_called) 
                    || BOX_VEC_FROM_RAW_REGEX.is_match(&function_called)
                    || VEC_FROM_RAW_REGEX.is_match(&function_called)
                    || BOX_FROM_RAW_NODE_GENERIC_REGEX.is_match(&function_called)
                    // vec
                    || function_called.contains("std::boxed::Box::<std::vec::Vec<char>>::from_raw")
                    // from raw msg sender
                    || function_called.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::from_raw")
                {
                    taint.entry(full_ret_place.clone())
                        .or_insert_with(HashSet::new)
                        .insert("assign".to_string());
                    let moved_var = extract_moved_var(details);
                    if !moved_var.is_empty() {
                        let moved_key = full_local_name(&moved_var);
                        taint.remove(&moved_key);
                    }
                } else if explicit_drop {
                    // Keep taint semantics aligned with the abstract memory:
                    // dropping a raw pointer does not free its pointee.
                    if !raw_pointer_drop {
                        if let Some(first_arg) = arguments.get(0) {
                            let dropped_name = extract_arg_name(&first_arg.arg);
                            let full_dropped = full_local_name(&dropped_name);

                            taint.entry(full_dropped.clone())
                                .or_insert_with(HashSet::new)
                                .insert("free".to_string());

                            if let Some(set) = taint.get_mut(&full_dropped) {
                                set.remove("assign");
                            }
                        }
                    }
                }
            }
        }
        MirTerminator::Drop { dropped_value, .. } => {
            let full_dropped = full_local_name(dropped_value);
            new_mem = update_state(new_mem, &full_dropped, CellValue::FREED);
            taint.entry(full_dropped.clone())
                .or_insert_with(HashSet::new)
                .insert("free".to_string());
            if let Some(set) = taint.get_mut(&full_dropped) {
                set.remove("assign");
            }
        }
        // Other terminators: no state change
        _ => {}
    }
    new_mem
}



// ----------------------------------------------------------------------
// LLVM / SVF ALLOCATION-PROVENANCE HELPERS
// ----------------------------------------------------------------------

/// Example replicated node:
/// `llvm::alloc_c_string::node12::rust::main::bb4`.
fn llvm_call_suffix_from_global_node_id(node_id: &str) -> Option<&str> {
    node_id.rfind("::rust::").map(|pos| &node_id[pos + 2..])
}

fn scoped_llvm_var(var_id: usize, node_id: &str) -> Name {
    match llvm_call_suffix_from_global_node_id(node_id) {
        Some(suffix) => format!("{}@{}", var_id, suffix),
        None => var_id.to_string(),
    }
}

fn scoped_llvm_ir_var(ir_id: usize, node_id: &str) -> Name {
    match llvm_call_suffix_from_global_node_id(node_id) {
        Some(suffix) => format!("%{}@{}", ir_id, suffix),
        None => format!("%{}", ir_id),
    }
}

fn is_c_malloc_family_alloc_call(info: &str) -> bool {
    info.contains("@malloc(") || info.contains("@calloc(")
}


/// SVF represents the malloc/calloc return pointer by an AddrStmt at the
/// allocator CallICFGNode. Its lhs is the returned pointer-valued SVF VarID.
fn llvm_c_allocation_result_vars(node: &LlvmJsonNode) -> Vec<usize> {
    if node.node_kind_string != "FunCallBlock"
        || !is_c_malloc_family_alloc_call(&node.info)
    {
        return Vec::new();
    }

    node.svf_statements
        .iter()
        .filter(|stmt| stmt.stmt_type == "AddrStmt")
        .filter_map(|stmt| stmt.lhs_var_id)
        .collect()
}

/// Relations that can carry a pointer/value provenance fact.
///
/// CmpStmt/BinaryOPStmt are excluded: a bool/integer computed from a pointer
/// is not an allocation handle.
fn llvm_provenance_flow_sources(stmt: &SvfStatement) -> Vec<usize> {
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

fn propagate_llvm_provenance(
    node_id: &str,
    stmt: &SvfStatement,
    mem: &mut AbstractMemory,
    taint: &mut TaintStateMap,
) {
    let Some(lhs) = stmt.result_var_id() else {
        return;
    };

    let carries_c_malloc = llvm_provenance_flow_sources(stmt)
        .into_iter()
        .map(|source| scoped_llvm_var(source, node_id))
        .filter_map(|source| taint.get(&source))
        .any(taint_has_c_malloc_origin);

    if !carries_c_malloc {
        return;
    }

    let lhs_name = scoped_llvm_var(lhs, node_id);
    taint
        .entry(lhs_name.clone())
        .or_default()
        .insert(TAINT_C_MALLOC_FAMILY.to_string());

    // Do not propagate the generic `assign` marker through every SVF
    // temporary/stack slot. Only allocation witnesses and the Rust FFI return
    // local are detector roots; intermediate SVF values carry provenance only.
    mem.assign_local_value(&lhs_name, CellValue::TOP);
}

/// Positive C malloc-family provenance is sufficient to report a possible
/// allocator / ownership contract mismatch at these Rust APIs. Absence of the
/// marker does NOT establish safety, and this phase does not claim otherwise.
fn c_malloc_rust_allocator_contract_warning(function_called: &str) -> Option<&'static str> {
    if is_cstring_from_raw_call(function_called) {
        Some(
            "CString::from_raw requires CString::into_raw provenance; \
foreign malloc-family ownership violates that contract",
        )
    } else if is_box_from_raw_call(function_called) {
        Some(
            "Box::from_raw requires Global-allocator provenance and the exact Box<T> \
layout/value contract; plain C malloc-family provenance does not prove those preconditions",
        )
    } else if is_vec_from_raw_parts_call(function_called) {
        Some(
            "Vec::from_raw_parts requires Global-allocator provenance plus exact \
layout/length/capacity invariants; plain C malloc-family provenance does not prove them",
        )
    } else if is_string_from_raw_parts_call(function_called) {
        Some(
            "String::from_raw_parts requires the String/Vec Global-allocator and \
layout/capacity/UTF-8 invariants; plain C malloc-family provenance does not prove them",
        )
    } else if is_raw_dealloc_call(function_called) {
        Some(
            "std::alloc::dealloc requires allocation by the Rust Global allocator \
with the matching Layout; plain C malloc-family provenance does not prove that contract",
        )
    } else {
        None
    }
}


/// Recognize the C malloc-family deallocator when the call occurs in Rust MIR.
///
/// A bare `free` is accepted only when CREMA's FFI scan found an `extern`
/// declaration named `free`.  This avoids classifying an unrelated Rust
/// function named `free` as libc's deallocator.
///
/// A `libc::...::free` path is accepted directly because the declaration lives
/// in the dependency crate, not in the analyzed crate's foreign module.
fn is_c_free_function(
    function_called: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    let f = function_called.trim();

    if f == "free" {
        return ffi_functions.contains("free");
    }

    f.contains("libc::") && f.ends_with("::free")
}

/// `transfer_call` receives the textual MIR call rather than the isolated
/// `function_called` field, so use an equivalent conservative recognizer there.
fn is_c_free_call_text(
    call_details: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    if call_details.contains("libc::") && call_details.contains("::free(") {
        return true;
    }

    ffi_functions.contains("free")
        && (call_details.trim_start().starts_with("free(")
            || call_details.contains(" free("))
}

/// Positive proof that `var` belongs to a free-flow component carrying
/// malloc/calloc provenance.
///
/// Absence of this marker is UNKNOWN.  It is not used as proof that C `free`
/// is invalid.
fn is_known_c_malloc_family(
    var: &Name,
    free_flow_keys: &BTreeMap<Name, String>,
    c_malloc_origin_vars: &HashSet<Name>,
) -> bool {
    if c_malloc_origin_vars.contains(var) {
        return true;
    }

    let Some(group) = lookup_free_flow_group(var, free_flow_keys) else {
        return false;
    };

    group
        .trim_matches(|c| c == '{' || c == '}')
        .split(',')
        .map(str::trim)
        .any(|member| c_malloc_origin_vars.contains(member))
}

static LLVM_FREE_IR_ARG_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"@free\([^%]*%([0-9]+)").unwrap());

fn llvm_free_ir_argument_id(info: &str) -> Option<usize> {
    LLVM_FREE_IR_ARG_REGEX
        .captures(info)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok())
}


/// Reachability in the already inlined GlobalICFG.
///
/// This is intentionally graph-semantic rather than source-line based: a C
/// `free` event has an LLVM node, while a later Rust use has a MIR node and the
/// two source spans are not comparable.  A path `free_node ->* use_node`
/// witnesses a possible execution in which the use occurs after the free.
fn icfg_may_reach(
    icfg: &GlobalICFGOrdered,
    from: &str,
    to: &str,
) -> bool {
    if from == to {
        return true;
    }

    let mut seen = BTreeSet::new();
    let mut work = VecDeque::from([from.to_string()]);
    seen.insert(from.to_string());

    while let Some(node) = work.pop_front() {
        for edge in icfg.icfg_edges.iter().filter(|e| e.source == node) {
            if edge.destination == to {
                return true;
            }
            if seen.insert(edge.destination.clone()) {
                work.push_back(edge.destination.clone());
            }
        }
    }

    false
}

fn c_malloc_has_use_after_inlined_free(
    icfg: &GlobalICFGOrdered,
    free_nodes: &BTreeSet<String>,
    use_nodes: &BTreeSet<String>,
) -> bool {
    free_nodes.iter().any(|free_node| {
        use_nodes
            .iter()
            .any(|use_node| icfg_may_reach(icfg, free_node, use_node))
    })
}

// ----------------------------------------------------------------------
// LLVM TRANSFER FUNCTIONS (super blueprint)
// ----------------------------------------------------------------------
fn transfer_llvm_node(
    node_id: &str,
    llvm_node: &LlvmJsonNode,
    in_mem: &AbstractMemory,
    in_taint: &TaintStateMap,
) -> (AbstractMemory, TaintStateMap) {
    let mut current_mem = in_mem.clone();
    let mut current_taint = in_taint.clone();

    for result in llvm_c_allocation_result_vars(llvm_node) {
        let result_name = scoped_llvm_var(result, node_id);
        current_mem.assign_local_value(&result_name, CellValue::TOP);
        let tags = current_taint.entry(result_name).or_default();
        tags.insert(TAINT_ASSIGN.to_string());
        tags.insert(TAINT_C_MALLOC_FAMILY.to_string());
    }

    for stmt in &llvm_node.svf_statements {
        propagate_llvm_provenance(
            node_id,
            stmt,
            &mut current_mem,
            &mut current_taint,
        );
        current_mem = apply_llvm_statement(&current_mem, stmt);
    }

    (current_mem, current_taint)
}


// JUST PROPAGATE THE CELL VALUE FROM MIR_VAR TO LLVM_VAR
// dummy call node’s transfer function propagates the cell value from the MIR variable to the LLVM variable by merging their alias sets (allocations)
// example: MIR variable: "Local(_2)" | LLVM variable: "10"
// they will share the same memory allocation
pub fn transfer_dummycall_node(dummycall_node: &DummyNode, in_mem: &AbstractMemory, in_taint: &TaintStateMap) -> (AbstractMemory, TaintStateMap) {
    let mut current_mem   = in_mem.clone();
    let mut current_taint = in_taint.clone();

    let positional = &dummycall_node.argument_bindings;
    if !positional.is_empty() {
        for binding in positional {
            let full_mir = full_local_name(&binding.mir_var);
            let full_llvm = binding.llvm_var.clone();

            // Each positional pair is joined independently. There is no
            // cross-argument equivalence closure.
            current_mem.propagate_cell_value(&full_mir, &full_llvm);
            current_mem.propagate_cell_value(&full_llvm, &full_mir);

            let taint_mir = current_taint.get(&full_mir).cloned().unwrap_or_default();
            let taint_llvm = current_taint.get(&full_llvm).cloned().unwrap_or_default();
            let joined = taint_mir.union(&taint_llvm).cloned().collect::<HashSet<_>>();
            current_taint.insert(full_mir.clone(), joined.clone());
            current_taint.insert(full_llvm, joined);
        }
    } else if let (Some(mir_var), Some(llvm_var)) =
        (&dummycall_node.mir_var, &dummycall_node.llvm_var)
    {
        // Historical single-argument artifacts.
        let full_mir  = full_local_name(mir_var);
        let full_llvm = llvm_var.to_string();

        current_mem.propagate_cell_value(&full_mir, &full_llvm);
        current_mem.propagate_cell_value(&full_llvm, &full_mir);

        let taint_mir  = current_taint.get(&full_mir).cloned().unwrap_or_default();
        let taint_llvm = current_taint.get(&full_llvm).cloned().unwrap_or_default();
        let joined     = taint_mir.union(&taint_llvm).cloned().collect::<HashSet<_>>();
        current_taint.insert(full_mir.clone(), joined.clone());
        current_taint.insert(full_llvm.clone(), joined);
    }

    (current_mem, current_taint)
}


// future
fn apply_llvm_statement(mem: &AbstractMemory, _stmt: &SvfStatement) -> AbstractMemory {
    let new_mem = mem.clone();
    /* 
    if let Some(lhs_var) = stmt.lhs_var_id {
        match stmt.stmt_type.as_str() {
            
            "alloca" | "malloc" => {
                new_mem.set_cell_value(&lhs_var.to_string(), CellValue::ALLOC);
            }
            
            "free" => {
                new_mem.set_cell_value(&lhs_var.to_string(), CellValue::FREED);
            }
            
            "assign" | "AddrStmt" => {
                if let Some(rhs_var) = stmt.rhs_var_id {
                    let v = mem.get_cell_value(&rhs_var.to_string()).unwrap_or(CellValue::BOTTOM);
                    new_mem.set_cell_value(&lhs_var.to_string(), v);
                }
            }// Helper to extract only the variable name string from an argument like "Local(_2) [mutable]"
fn extract_arg_name(arg: &str) -> String {
    if let Some(start) = arg.find('(') {
        if let Some(end) = arg.find(')') {
            return arg[start + 1..end].to_string();
        }
    }
    arg.to_string()
}


            _ => {}
        }
    }
*/
    new_mem
}


// ----------------------------------------------------------------------
// FIXED-POINT ANALYSIS
// ----------------------------------------------------------------------

//////// LAST VISITED NODE: from fixed_point to detect memory issues for filtering \\\\\\\\\\\\

thread_local! {
    // storing last visited node in a thread-local variable (to pass it from fixed point to detect mem issues for filtering)
    static LAST_VISITED_NODE: RefCell<Option<String>> = RefCell::new(None);
}
pub fn set_last_visited(node: String) {
    LAST_VISITED_NODE.with(|cell| {
        *cell.borrow_mut() = Some(node);
    });
}

// get (by coloning) last visited node
pub fn get_last_visited() -> Option<String> {
    LAST_VISITED_NODE.with(|cell| cell.borrow().clone())
}

///////// ENTRY POINT: from STDIN to fixed_point  \\\\\\\\\\\\
thread_local! {
    // default entry‐point = rust::main::bb0
    static ENTRYPOINT: RefCell<String> = RefCell::new("rust::main::bb0".to_string());
}

pub fn set_entrypoint(ep: String) {
    ENTRYPOINT.with(|e| *e.borrow_mut() = ep);
}

fn get_entrypoint() -> String {
    ENTRYPOINT.with(|e| e.borrow().clone())
}



#[derive(Clone, Default)]
struct InternalCallContinuation {
    caller_mem: AbstractMemory,
    caller_taint: TaintStateMap,
    initialized: bool,
}

impl InternalCallContinuation {
    /// Join a newly observed caller snapshot into this callsite summary.
    ///
    /// A worklist analysis may revisit the same callsite many times.  The
    /// continuation therefore stores a monotone summary rather than a LIFO
    /// frame belonging to one particular traversal order.
    fn join_caller_snapshot(
        &mut self,
        mem: &AbstractMemory,
        taint: &TaintStateMap,
    ) -> bool {
        if !self.initialized {
            self.caller_mem = mem.clone();
            self.caller_taint = taint.clone();
            self.initialized = true;
            return true;
        }

        let joined_mem = self.caller_mem.union(mem);
        let joined_taint =
            join_taint_maps(&self.caller_taint, taint, &joined_mem);
        let changed =
            joined_mem != self.caller_mem || joined_taint != self.caller_taint;

        if changed {
            self.caller_mem = joined_mem;
            self.caller_taint = joined_taint;
        }

        changed
    }
}

/// Recover the Rust function owning a MIR node id.
///
/// `rsplit_once` is required because closure names themselves contain `::`,
/// e.g. `rust::main::{closure#0}::bb4`.
fn rust_function_from_mir_node_id(node_id: &str) -> Option<&str> {
    let rest = node_id.strip_prefix("rust::")?;
    let (function, block) = rest.rsplit_once("::bb")?;
    if function.is_empty() || block.parse::<usize>().is_err() {
        return None;
    }
    Some(function)
}

fn internal_call_for_dummy_call<'a>(
    icfg: &'a GlobalICFGOrdered,
    node_id: &str,
) -> Option<&'a RustCallMetadata> {
    icfg.rust_calls
        .iter()
        .find(|call| call.dummy_call_node == node_id)
}

fn internal_calls_for_callee<'a>(
    icfg: &'a GlobalICFGOrdered,
    callee: &str,
) -> Vec<&'a RustCallMetadata> {
    icfg.rust_calls
        .iter()
        .filter(|call| call.callee_function == callee)
        .collect()
}

fn mir_argument_local(argument: &crate::structs::MirCallArgument) -> Option<Name> {
    first_call_local_from_details(&argument.arg).or_else(|| {
        LOCAL_TOKEN_RE
            .find(&argument.arg)
            .map(|m| full_local_name(m.as_str()))
    })
}

/// Apply a conservative actual->formal binding on the canonical activation
/// edge.  AbstractMemory is still legacy/unscoped, so the caller state is kept
/// as MAY context and formal locals receive the corresponding actual value.
/// This can lose precision but cannot hide an actual heap identity.
fn bind_internal_actuals(
    call: &RustCallMetadata,
    function: &crate::structs::RustFunctionMetadata,
    memory: &AbstractMemory,
    taint: &TaintStateMap,
) -> (AbstractMemory, TaintStateMap) {
    let mut out_mem = memory.clone();
    let mut out_taint = taint.clone();
    for (index, argument) in call.arguments.iter().take(function.arg_count).enumerate() {
        let Some(actual) = mir_argument_local(argument) else { continue; };
        let formal = format!("Local(_{})", index + 1);
        let value = out_mem.get_cell_value(&actual);
        if value != CellValue::BOTTOM && out_mem.get_allocation(&actual).is_some() {
            out_mem.propagate_cell_value(&actual, &formal);
        }
        if let Some(tags) = out_taint.get(&actual).cloned() {
            out_taint.entry(formal).or_default().extend(tags);
        }
    }
    (out_mem, out_taint)
}

pub fn fixed_point_analysis(icfg: &GlobalICFGOrdered) -> (AbstractState, TaintState) {

    let mut succs_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut outgoing_edges: BTreeMap<String, Vec<IcfgEdge>> = BTreeMap::new();
    let mut edge_transfer_only: BTreeSet<(String, String)> = BTreeSet::new();
    for edge in &icfg.icfg_edges {
        outgoing_edges
            .entry(edge.source.clone())
            .or_default()
            .push(edge.clone());
        succs_map
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.destination.clone());
        if matches!(
            edge.label.as_deref(),
            Some("dummyCall -> Rust Entry") | Some("Rust Return -> dummyRet")
        ) {
            edge_transfer_only.insert((edge.source.clone(), edge.destination.clone()));
        }
    }

    // 1) init worklist
    let mut abs_state   = AbstractState::default();
    let mut taint_state = TaintState::default();
    let mut worklist: BTreeSet<String> = BTreeSet::new();
    // One monotone caller snapshot per ordinary internal Rust callsite.
    // This replaces the traversal-order-dependent global LIFO call stack.
    let mut internal_continuations: BTreeMap<String, InternalCallContinuation> =
        BTreeMap::new();
    let mut visit_order: Vec<String> = Vec::new();

    // entrypoint
    let entry = get_entrypoint();
    println!("entrypoint: {}", entry);

    {
        let node = get_node_by_id(icfg, &entry);
        let (m, t) = transfer_function(
            &entry,
            &node,
            &abs_state.get(&entry).unwrap_or_default(),
            &taint_state.get(&entry).cloned().unwrap_or_default()
        );
        abs_state.insert(entry.clone(), m);
        taint_state.insert(entry.clone(), t);
    }
    worklist.insert(entry.clone());

    // 2) Worklist loop: extract the smallest
    while let Some(current) = {
        // take and remove lexicalgrafix smallest
        worklist.iter().next().cloned().map(|n| {
            worklist.remove(&n);
            n
        })
    } {
        visit_order.push(current.clone());
        let curr_mem   = abs_state.get(&current).unwrap_or_default();
        let curr_taint = taint_state.get(&current).cloned().unwrap_or_default();

        // 2.a) intraprocedural / summary edges.  The frozen profile keeps the
        // historical successor-deduplicated traversal exactly.  The A3 profile
        // switches to edge-level traversal because normal and unwind edges from
        // the same MIR block can carry different post-states.
        if panic_unwind_lifecycle_v1_enabled() {
            if let Some(edges) = outgoing_edges.get(&current) {
                for edge in edges {
                    let succ = &edge.destination;
                    if edge_transfer_only.contains(&(current.clone(), succ.clone())) {
                        continue;
                    }

                    let mut edge_mem = curr_mem.clone();
                    let mut edge_taint = curr_taint.clone();

                    // A3.6 custom Drop bodies execute before ordinary drop-glue
                    // completion.  The represented destructor returns through a
                    // DummyRet; only on that normal return do we apply the
                    // existing Drop completion summary (FREED for tracked
                    // ownership plus taint).  This preserves both the explicit
                    // user destructor effects and CREMA's recursive-field/drop-
                    // glue abstraction without applying either effect early.
                    if matches!(
                        edge.label.as_deref(),
                        Some("dummyRet -> Rust Drop Continuation")
                    ) {
                        if let Some(call) = icfg
                            .rust_calls
                            .iter()
                            .find(|call| call.dummy_ret_node == current)
                        {
                            if let GlobalICFGNode::Mir(drop_site) =
                                get_node_by_id(icfg, &call.call_node)
                            {
                                if let Some(term @ MirTerminator::Drop { .. }) =
                                    drop_site.terminator.as_ref()
                                {
                                    edge_mem = apply_mir_terminator_with_extensions(
                                        &edge_mem,
                                        &mut edge_taint,
                                        term,
                                    );
                                }
                            }
                        }
                    }

                    if let GlobalICFGNode::Mir(source_bb) = get_node_by_id(icfg, &current) {
                        if let Some(term) = source_bb.terminator.as_ref() {
                            edge_mem = apply_mir_terminator_for_edge_with_extensions(
                                &edge_mem,
                                &mut edge_taint,
                                term,
                                edge,
                            );
                        }
                    }

                    let node = get_node_by_id(icfg, succ);
                    let (new_mem, new_taint) = transfer_function(
                        succ,
                        &node,
                        &edge_mem,
                        &edge_taint,
                    );

                    let old_mem   = abs_state.get(succ).unwrap_or_default();
                    let old_taint = taint_state.get(succ).cloned().unwrap_or_default();
                    let joined_mem = old_mem.union(&new_mem);
                    let joined_taint = join_taint_maps(&old_taint, &new_taint, &joined_mem);
                    let first = !abs_state.state_map.contains_key(succ) && !taint_state.contains_key(succ);
                    if first || joined_mem != old_mem || joined_taint != old_taint {
                        abs_state.insert(succ.clone(), joined_mem);
                        taint_state.insert(succ.clone(), joined_taint);
                        worklist.insert(succ.clone());
                    }
                }
            }
        } else if let Some(succs) = succs_map.get(&current) {
            // Frozen v6O/v6N traversal: do not alter ordering or duplicate-edge
            // behavior when the A3 profile is disabled.
            for succ in succs {
                if edge_transfer_only.contains(&(current.clone(), succ.clone())) {
                    continue;
                }
                let node = get_node_by_id(icfg, succ);
                let (new_mem, new_taint) = transfer_function(succ, &node, &curr_mem, &curr_taint);

                let old_mem   = abs_state.get(succ).unwrap_or_default();
                let old_taint = taint_state.get(succ).cloned().unwrap_or_default();
                let joined_mem = old_mem.union(&new_mem);
                let joined_taint = join_taint_maps(&old_taint, &new_taint, &joined_mem);
                let first = !abs_state.state_map.contains_key(succ) && !taint_state.contains_key(succ);
                if first || joined_mem != old_mem || joined_taint != old_taint {
                    abs_state.insert(succ.clone(), joined_mem);
                    taint_state.insert(succ.clone(), joined_taint);
                    worklist.insert(succ.clone());
                }
            }
        }

        // 2.b) edge-specific transfer on the *canonical* Rust call/return edges.
        // Metadata carries bindings only; it must never create a transition
        // absent from `icfg_edges`.
        if let Some(call) = internal_call_for_dummy_call(icfg, &current) {
            let Some(function) = icfg.rust_functions.get(&call.callee_function) else {
                continue;
            };
            if !succs_map
                .get(&current)
                .is_some_and(|succs| succs.contains(&function.entry_node))
            {
                panic!(
                    "v6K canonical ICFG invariant violated: metadata activation {} -> {} has no edge",
                    current, function.entry_node
                );
            }

            let continuation_changed = internal_continuations
                .entry(call.call_node.clone())
                .or_default()
                .join_caller_snapshot(&curr_mem, &curr_taint);

            let (bound_mem, bound_taint) =
                bind_internal_actuals(call, function, &curr_mem, &curr_taint);
            let entry_node = get_node_by_id(icfg, &function.entry_node);
            let (candidate_mem, candidate_taint) = transfer_function(
                &function.entry_node,
                &entry_node,
                &bound_mem,
                &bound_taint,
            );
            let old_mem = abs_state.get(&function.entry_node).unwrap_or_default();
            let old_taint = taint_state
                .get(&function.entry_node)
                .cloned()
                .unwrap_or_default();
            let joined_mem = old_mem.union(&candidate_mem);
            let joined_taint = join_taint_maps(&old_taint, &candidate_taint, &joined_mem);
            let first = !abs_state.state_map.contains_key(&function.entry_node)
                && !taint_state.contains_key(&function.entry_node);
            if first || joined_mem != old_mem || joined_taint != old_taint {
                abs_state.insert(function.entry_node.clone(), joined_mem);
                taint_state.insert(function.entry_node.clone(), joined_taint);
                worklist.insert(function.entry_node.clone());
            }

            if continuation_changed {
                for ret in &function.return_nodes {
                    if abs_state.state_map.contains_key(ret) {
                        worklist.insert(ret.clone());
                    }
                }
            }
        }

        if let GlobalICFGNode::Mir(ref bb) = get_node_by_id(icfg, &current) {
            if matches!(&bb.terminator, Some(MirTerminator::Return { .. })) {
                if let Some(callee) = rust_function_from_mir_node_id(&current) {
                    let callee_mem = abs_state.get(&current).unwrap_or_default();
                    let callee_taint = taint_state.get(&current).cloned().unwrap_or_default();

                    for call in internal_calls_for_callee(icfg, callee) {
                        let Some(continuation) = internal_continuations.get(&call.call_node) else {
                            continue;
                        };
                        if !succs_map
                            .get(&current)
                            .is_some_and(|succs| succs.contains(&call.dummy_ret_node))
                        {
                            panic!(
                                "v6K canonical ICFG invariant violated: metadata return {} -> {} has no edge",
                                current, call.dummy_ret_node
                            );
                        }

                        let candidate_mem = continuation.caller_mem.union(&callee_mem);
                        let candidate_taint = join_taint_maps(
                            &continuation.caller_taint,
                            &callee_taint,
                            &candidate_mem,
                        );
                        let ret_node = call.dummy_ret_node.clone();
                        let dummy = get_node_by_id(icfg, &ret_node);
                        let (mapped_mem, mapped_taint) = transfer_function(
                            &ret_node,
                            &dummy,
                            &candidate_mem,
                            &candidate_taint,
                        );

                        let old_mem = abs_state.get(&ret_node).unwrap_or_default();
                        let old_taint = taint_state.get(&ret_node).cloned().unwrap_or_default();
                        let joined_mem = old_mem.union(&mapped_mem);
                        let joined_taint = join_taint_maps(&old_taint, &mapped_taint, &joined_mem);
                        let first = !abs_state.state_map.contains_key(&ret_node)
                            && !taint_state.contains_key(&ret_node);
                        if first || joined_mem != old_mem || joined_taint != old_taint {
                            abs_state.insert(ret_node.clone(), joined_mem);
                            taint_state.insert(ret_node.clone(), joined_taint);
                            worklist.insert(ret_node);
                        }
                    }
                }
            }
        }
    }
    if let Some(last) = visit_order.iter().rev().find(|n| !n.starts_with("dummy")) {
        set_last_visited(last.clone());
    }

    (abs_state, taint_state)
}



#[cfg(test)]
mod phase6b_interprocedural_protocol_tests {
    use super::*;
    use crate::structs::{GlobalICFGOrdered, MirCallArgument, RustCallMetadata};

    fn call(call_node: &str, callee: &str, dummy_ret: &str) -> RustCallMetadata {
        RustCallMetadata {
            caller_function: "main".to_string(),
            call_node: call_node.to_string(),
            callee_function: callee.to_string(),
            dummy_call_node: format!("dummyCall::{call_node}"),
            dummy_ret_node: dummy_ret.to_string(),
            arguments: vec![MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            return_place: "_0".to_string(),
            return_node: "rust::main::bb9".to_string(),
            is_closure: false,
        }
    }

    #[test]
    fn rust_function_parser_is_scope_aware_for_closures() {
        assert_eq!(
            rust_function_from_mir_node_id("rust::main::{closure#0}::bb4"),
            Some("main::{closure#0}")
        );
        assert_eq!(
            rust_function_from_mir_node_id("rust::foo::bb12"),
            Some("foo")
        );
        assert_eq!(rust_function_from_mir_node_id("dummyRet::x"), None);
    }

    #[test]
    fn return_matching_is_by_callee_not_lifo_order() {
        let icfg = GlobalICFGOrdered {
            ordered_nodes: Vec::new(),
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::new(),
            rust_calls: vec![
                call("rust::main::bb1", "foo", "dummyRet::foo1"),
                call("rust::main::bb2", "bar", "dummyRet::bar"),
                call("rust::main::bb3", "foo", "dummyRet::foo2"),
            ],
        };

        let matched = internal_calls_for_callee(&icfg, "foo");
        let ids: Vec<_> = matched.iter().map(|c| c.call_node.as_str()).collect();
        assert_eq!(ids, vec!["rust::main::bb1", "rust::main::bb3"]);
    }

    #[test]
    fn v6l_same_callsite_fanout_is_matched_by_unique_dummy_nodes() {
        let mut a = call("rust::main::bb5", "impl_a::new", "dummyRet::main::bb5::instance0");
        a.dummy_call_node = "dummyCall::main::bb5::instance0".into();
        let mut b = call("rust::main::bb5", "impl_b::new", "dummyRet::main::bb5::instance1");
        b.dummy_call_node = "dummyCall::main::bb5::instance1".into();
        let icfg = GlobalICFGOrdered {
            ordered_nodes: Vec::new(),
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::new(),
            rust_calls: vec![a, b],
        };

        assert_eq!(
            internal_call_for_dummy_call(&icfg, "dummyCall::main::bb5::instance0")
                .unwrap().callee_function,
            "impl_a::new"
        );
        assert_eq!(
            internal_call_for_dummy_call(&icfg, "dummyCall::main::bb5::instance1")
                .unwrap().callee_function,
            "impl_b::new"
        );
    }

    #[test]
    fn continuation_snapshot_join_is_monotone() {
        let mut continuation = InternalCallContinuation::default();
        let mut allocated = AbstractMemory::default();
        allocated.set_cell_value(&"Local(_1)".to_string(), CellValue::ALLOC);
        let empty_taint = TaintStateMap::new();

        assert!(continuation.join_caller_snapshot(&allocated, &empty_taint));
        assert!(!continuation.join_caller_snapshot(&allocated, &empty_taint));

        let mut freed = AbstractMemory::default();
        freed.set_cell_value(&"Local(_1)".to_string(), CellValue::FREED);
        assert!(continuation.join_caller_snapshot(&freed, &empty_taint));
        assert_eq!(
            continuation
                .caller_mem
                .get_cell_value(&"Local(_1)".to_string()),
            CellValue::TOP
        );
    }

    #[test]
    fn higher_order_rust_call_metadata_drives_closure_capture_binding() {
        use crate::structs::{
            GlobalICFGNode, MirBasicBlock, MirStatement, SourceInfoData,
        };

        let closure_env_stmt = MirStatement {
            source_info: SourceInfoData {
                span: "test.rs:1:1:1:1 (#0)".to_string(),
                scope: "scope[0]".to_string(),
            },
            kind: "Assign".to_string(),
            details: "Assign((Local(_3), {closure@test.rs:1:1: 1:3} { ptr: move _4 }))".to_string(),
            place: Some("Local(_3)".to_string()),
            is_mutable: Some(false),
            rvalue: Some(
                "{closure@test.rs:1:1: 1:3} { ptr: move _4 }".to_string(),
            ),
        };

        // The MIR terminator does not call the closure directly.  This models a
        // higher-order library summary (spawn/consumer) whose canonical callback
        // relation is carried solely by `rust_calls`.
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(
                "rust::main::bb0".to_string(),
                GlobalICFGNode::Mir(MirBasicBlock {
                    block_id: 0,
                    statements: vec![closure_env_stmt],
                    terminator: None,
                }),
            )],
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::new(),
            rust_calls: vec![RustCallMetadata {
                caller_function: "main".to_string(),
                call_node: "rust::main::bb1".to_string(),
                callee_function: "main::{closure#0}".to_string(),
                dummy_call_node: "dummyCall::main::bb1::callback0".to_string(),
                dummy_ret_node: "dummyRet::main::bb1::callback0".to_string(),
                arguments: vec![MirCallArgument {
                    arg: "Local(_3)".to_string(),
                    is_mutable: Some(false),
                }],
                return_place: String::new(),
                return_node: "rust::main::bb2".to_string(),
                is_closure: true,
            }],
        };

        let bindings = build_closure_capture_bindings(&icfg);
        let captures = bindings
            .get("main::{closure#0}")
            .expect("canonical higher-order rust_call must bind the closure environment");

        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].by_value_sources,
            BTreeSet::from(["Local(_4)".to_string()])
        );
        assert!(captures[0].stack_ref_targets.is_empty());
    }
}

#[cfg(test)]
mod panic_unwind_lifecycle_v1_tests {
    use super::*;

    fn edge(label: &str, destination: &str) -> IcfgEdge {
        IcfgEdge {
            source: "rust::main::bb0".to_string(),
            destination: destination.to_string(),
            label: Some(label.to_string()),
            source_label: None,
            destination_label: None,
        }
    }

    #[test]
    fn drop_normal_and_unwind_have_distinct_post_states() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&"Local(_1)".to_string(), CellValue::ALLOC);
        let term = MirTerminator::Drop {
            details: "drop(_1) -> [return: bb1, unwind: bb2]".to_string(),
            source_info: "test.rs:1:1:1:1 (#0)".to_string(),
            return_target: "bb1".to_string(),
            unwind_target: "cleanup(bb2)".to_string(),
            dropped_value: "_1".to_string(),
            is_mutable: true,
            deallocator_evidence: None,
        };

        let mut normal_taint = TaintStateMap::new();
        let normal = apply_mir_terminator_for_edge_enabled(
            &mem,
            &mut normal_taint,
            &term,
            &edge("Drop return", "rust::main::bb1"),
        );
        assert_eq!(normal.get_cell_value(&"Local(_1)".to_string()), CellValue::FREED);

        let mut unwind_taint = TaintStateMap::new();
        let unwind = apply_mir_terminator_for_edge_enabled(
            &mem,
            &mut unwind_taint,
            &term,
            &edge("Drop unwind", "rust::main::bb2"),
        );
        assert_eq!(unwind.get_cell_value(&"Local(_1)".to_string()), CellValue::TOP);
        assert!(unwind_taint
            .get("Local(_1)")
            .is_some_and(|tags| tags.contains("unwind_partial_drop")));
    }

    #[test]
    fn represented_custom_drop_enters_body_before_drop_completion() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&"Local(_1)".to_string(), CellValue::ALLOC);
        let term = MirTerminator::Drop {
            details: "drop(_1) -> [return: bb1, unwind: bb2]".to_string(),
            source_info: "test.rs:1:1:1:1 (#0)".to_string(),
            return_target: "bb1".to_string(),
            unwind_target: "cleanup(bb2)".to_string(),
            dropped_value: "_1".to_string(),
            is_mutable: true,
            deallocator_evidence: None,
        };

        let mut taint = TaintStateMap::new();
        let entered = apply_mir_terminator_for_edge_enabled(
            &mem,
            &mut taint,
            &term,
            &edge("Rust Drop -> dummyCall", "dummyCall::rust::main::bb0"),
        );

        assert_eq!(
            entered.get_cell_value(&"Local(_1)".to_string()),
            CellValue::ALLOC,
            "user Drop::drop must observe the pre-drop state; completion happens after its normal return"
        );
        assert!(taint.get("Local(_1)").is_none());
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

// ----------------------------------------------------------------------
// MEMORY ISSUE DETECTION
// ----------------------------------------------------------------------
fn multiset_add(set: &mut MultiSet, var: Name) {
    *set.entry(var).or_insert(0) += 1;
}

fn normalize_name(name: &Name) -> Name {
    // trim su spazi e sulle virgolette
    name.trim_matches(|c: char| c == '"' || c.is_whitespace()).to_string()
}

/// Data una variabile normalizzata, cerca se essa è contenuta in uno dei gruppi free flow.
/// I gruppi sono rappresentati come stringhe del tipo {"9", "Local(_2)", "Local(_5)"}.
/// Se viene trovata una corrispondenza, la funzione restituisce la chiave del gruppo
/// altrimenti None.

fn lookup_free_flow_group(var: &Name, free_flow_keys: &BTreeMap<Name, String>) -> Option<String> {
    let norm = normalize_name(var);
    for group_str in free_flow_keys.values() {
        let members: Vec<&str> = group_str
            .trim_matches(|c| c == '{' || c == '}')
            .split(',')
            .map(str::trim)
            .collect();
        if members.iter().any(|&m| m == norm) {
            return Some(group_str.clone());
        }
    }
    None
}



fn span_to_line(span: &str) -> Option<usize> {
    // split sover':'; es. "/…/main.rs:10:40: 10:41 (#0)" -> ["…/main.rs", "10", "40", " 10", "41 (#0)"]
    let parts: Vec<&str> = span.split(':').collect();
    if parts.len() >= 4 {
        // parts.len() == 5 eg, want parts[1] == "10"
        // like parts[parts.len() - 4]
        if let Ok(line) = parts[parts.len() - 4].parse::<usize>() {
            return Some(line);
        }
    }
    None
}

#[derive(Debug, Clone)]
enum FreeKind {
    LLVM,
    /// Matching malloc/calloc -> free observed in LLVM/SVF.
    CMallocFree,
    /// C `free` called directly from a Rust MIR call site.
    CMallocFreeRustCall,
    Drop,
    StdDealloc,
}

#[derive(Debug, Clone)]
struct VarInfo {
    llvm_free: usize,
    drop_free: usize,
    /// Explicit C `free` calls observed at Rust MIR call sites.
    c_free_mir: usize,
    used: bool,
    use_span: Option<String>,
    free_span: Option<(String, FreeKind)>, // first free span and its kind
    /// ICFG nodes at which the allocation/pointee is used.
    use_nodes: BTreeSet<String>,
    /// Inlined LLVM nodes containing matching C `free` events.
    llvm_free_nodes: BTreeSet<String>,
    /// MIR nodes containing a direct matching C `free` call.  Keeping node
    /// identity lets UAF detection use graph reachability rather than source
    /// line order.
    c_free_mir_nodes: BTreeSet<String>,
}

impl VarInfo {
    fn new() -> Self {
        Self {
            llvm_free: 0,
            drop_free: 0,
            c_free_mir: 0,
            used: false,
            use_span: None,
            free_span: None,
            use_nodes: BTreeSet::new(),
            llvm_free_nodes: BTreeSet::new(),
            c_free_mir_nodes: BTreeSet::new(),
        }
    }
 
    // if both LLVM and Drop frees are registered, return the maximum of the two,
    // otherwise return the sum.
    // this way an LLVM free + a Drop free produces effective_free == 1.
    fn effective_free(&self) -> usize {
        // Preserve CREMA's historical LLVM-vs-Drop deduplication.  A direct
        // Rust call to C `free` is nevertheless a separate explicit
        // deallocation event (so `free(p); free(p);` counts as two).
        let legacy = if self.llvm_free > 0 && self.drop_free > 0 {
            std::cmp::max(self.llvm_free, self.drop_free)
        } else {
            self.llvm_free + self.drop_free
        };

        legacy + self.c_free_mir
    }
}
/// Resolve a MIR local mentioned by a Drop terminator to an already tracked
/// detector key after the complete alias/free-flow relation is available.
///
/// The detector historically accepted both `_N` and `Local(_N)` spellings.
/// If neither direct spelling is present, use the completed free-flow group and
/// pick a tracked member of that group.  The result is deterministic because
/// BTreeMap iteration is ordered.
fn resolve_drop_tracking_key(
    dropped: &Name,
    var_info: &BTreeMap<Name, VarInfo>,
    free_flow_keys: &BTreeMap<Name, String>,
) -> Option<Name> {
    let norm = normalize_name(dropped);
    let wrapped = normalize_name(&format!("Local({})", norm));

    for candidate in [&norm, &wrapped] {
        if var_info.contains_key(candidate) {
            return Some(candidate.clone());
        }
    }

    for candidate in [&norm, &wrapped] {
        if let Some(group) = lookup_free_flow_group(candidate, free_flow_keys) {
            let members = group
                .trim_matches(|c| c == '{' || c == '}')
                .split(',')
                .map(str::trim);

            for member in members {
                let member = normalize_name(&member.to_string());
                if var_info.contains_key(&member) {
                    return Some(member);
                }
            }
        }
    }

    None
}


/// Choose the single logical source required by CREMA's historical
/// allocation-identity map without making Phase-5 C-origin behavior depend on
/// Phi/Select operand serialization order.
///
/// If any candidate already carries positive C malloc-family provenance, choose
/// a deterministic C-origin representative. Otherwise preserve the historical
/// first-source policy to avoid changing the frozen Phase-4.4 semantics.
fn preferred_provenance_source(
    source_names: &[Name],
    c_malloc_origin_vars: &HashSet<Name>,
) -> Option<Name> {
    let mut c_candidates: Vec<Name> = source_names
        .iter()
        .filter(|name| c_malloc_origin_vars.contains(*name))
        .cloned()
        .collect();
    c_candidates.sort();
    c_candidates.dedup();

    c_candidates
        .into_iter()
        .next()
        .or_else(|| source_names.first().cloned())
}

// DETECTION OF MEMORY ISSUES
pub fn detect_mem_issues(icfg: &GlobalICFGOrdered, taint_states: &TaintState, abs_state: &AbstractState) -> (MultiSet, MultiSet, MultiSet) {

    // SETUP
    // Numeric LLVM/SVF identifiers are scoped by the replicated FFI call site.
    let mut svf_to_name: BTreeMap<Name, Name> = BTreeMap::new();
    let mut ir_to_name: BTreeMap<Name, Name> = BTreeMap::new();
    let mut var_info = BTreeMap::new();

    //////////////////////////////////////////////////////////////////////////////
    // Cross-language allocator diagnostics.
    let mut llvm_warnings: Vec<(String, String)> = Vec::new();
    let mut ffi_allocator_warnings: Vec<(String, String, String)> = Vec::new();
    let mut c_malloc_origin_vars: HashSet<Name> = HashSet::new();
    let mut c_free_mismatch_warnings: Vec<(String, String)> = Vec::new();

    // Used only to disambiguate a bare user-declared foreign `free` from an
    // unrelated Rust function with the same name.
    let ffi_functions = load_ffi_functions("./ffi_functions.json")
        .unwrap_or_default();
    //////////////////////////////////////////////////////////////////////////////

    // v6K legacy-detector boundary: analyze exactly the subgraph reachable
    // from the selected entry.  The historical detector seeded its worklist
    // with *every* ICFG node and merely moved main to the front, which allowed
    // unreachable helper/test functions to create false positives.
    let selected_entry = get_entrypoint();
    let mut reachable: BTreeSet<String> = BTreeSet::new();
    let mut reach_worklist: VecDeque<String> = VecDeque::new();
    reachable.insert(selected_entry.clone());
    reach_worklist.push_back(selected_entry.clone());
    while let Some(src) = reach_worklist.pop_front() {
        let mut succs: Vec<String> = icfg.icfg_edges.iter()
            .filter(|edge| edge.source == src)
            .map(|edge| edge.destination.clone())
            .collect();
        succs.sort();
        succs.dedup();
        for dst in succs {
            if reachable.insert(dst.clone()) {
                reach_worklist.push_back(dst);
            }
        }
    }

    for (block, vars) in taint_states.iter() {
        if !reachable.contains(block) {
            continue;
        }
        for (var, markers) in vars.iter() {
            if markers.contains(TAINT_ASSIGN) {
                let norm = normalize_name(var);
                println!(
                    "Tracking variable '{}' from block '{}' (markers: {:?})",
                    norm, block, markers
                );
                var_info.entry(norm.clone()).or_insert(VarInfo::new());
                if taint_has_c_malloc_origin(markers) {
                    c_malloc_origin_vars.insert(norm);
                }
            }
        }
    }

    // Deterministic reachable-only worklist.  `selected_entry` is the same
    // canonical entry used by fixed_point_analysis and CQPL export.
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut worklist: VecDeque<String> = VecDeque::new();
    if reachable.contains(&selected_entry) {
        worklist.push_back(selected_entry.clone());
    } else {
        println!("Selected entry '{}' is not present in the reachable ICFG", selected_entry);
    }

    // free-flow e union-find
    let mut free_flow_parent = BTreeMap::new();
    let mut processed_llvm_free = BTreeSet::new();

    for var in &c_malloc_origin_vars {
        free_flow_parent.insert(var.clone(), var.clone());
    }

    // MIR Drop nodes may be encountered before the allocation-producing
    // from_raw node because GlobalICFG traversal order is not a semantic
    // execution order.  Record normal drops first and resolve them after the
    // complete alias/free-flow relation has been constructed.
    let mut pending_mir_drops: Vec<(Name, String)> = Vec::new();
    let mut pending_std_deallocs: Vec<(Name, String)> = Vec::new();
    let mut pending_c_free_calls: Vec<(Name, String, String)> = Vec::new();
    // Store the scoped textual LLVM IR argument key rather than resolving it
    // immediately.  The SVF->logical mapping and MIR<->LLVM alias relation are
    // only complete after the whole GlobalICFG traversal.
    let mut pending_llvm_free_calls: Vec<(Name, String, String)> = Vec::new();
    let mut pending_pointer_uses: Vec<(Name, String, String)> = Vec::new();


    // Directed MAY relation for references to MIR stack places. This is
    // intentionally separate from heap free-flow/alias equivalence.
    let mut stack_ref_targets: BTreeMap<Name, BTreeSet<Name>> = BTreeMap::new();

    // Closure environment semantics are precomputed from MIR statements and
    // the concrete GlobalICFG closure-call edges. This makes capture recovery
    // independent from the incidental detector worklist order.
    let closure_capture_bindings = build_closure_capture_bindings(icfg);

    // find & union per Name
    let find = |x: &Name, parent: &mut BTreeMap<Name, Name>| -> Name {
        let mut rep = normalize_name(x);
        while let Some(p) = parent.get(&rep) {
            if *p == rep { break; }
            rep = p.clone();
        }
        rep
    };
    let union = |x: &Name, y: &Name, parent: &mut BTreeMap<Name, Name>| {
        let rx = find(x, parent);
        let ry = find(y, parent);
        if rx != ry { parent.insert(ry, rx); }
    };

    // ICFG traversal
    while let Some(node_id) = worklist.pop_front() {
        if visited.contains(&node_id) { continue; }
        //println!("Processing node: '{}'", node_id);
        visited.insert(node_id.clone());
        let node = get_node_by_id(icfg, &node_id);
        match node {
            GlobalICFGNode::Mir(mir_block) => {
               // println!("-> MIR node (block_id: {}) with {} statement(s)", mir_block.block_id, mir_block.statements.len());
                for stmt in &mir_block.statements {
                    // A pointer cast can introduce the exact MIR local later
                    // passed to a direct foreign `free` (for example `_4` from
                    // `raw.cast::<c_void>()`). Register both endpoints in the
                    // detector relation. Do not union them yet: the global
                    // AbstractState reconciliation below will connect them only
                    // when transfer semantics actually proved an alias.
                    if let Some((dest, source)) =
                        detector_pointer_cast_participants(stmt)
                    {
                        free_flow_parent
                            .entry(dest.clone())
                            .or_insert(dest);
                        free_flow_parent
                            .entry(source.clone())
                            .or_insert(source);
                    }

                    // Record direct stack-place references without merging them
                    // into heap free-flow.
                    if let Some(dest_raw) = &stmt.place {
                        let dest = canonical_mir_local(dest_raw);

                        // `deref_copy ((*_1).N: Ty)` reads field N from the
                        // closure environment.  The call graph prepass tells us
                        // what that field captured.
                        if let Some(rvalue) = stmt.rvalue.as_deref() {
                            if let Some(field_idx) =
                                closure_field_copy_for_deref_index(rvalue)
                            {
                                if let Some(scope) =
                                    mir_function_scope_from_node_id(&node_id)
                                {
                                    if let Some(binding) =
                                        closure_capture_bindings
                                            .get(&scope)
                                            .and_then(|v| v.get(field_idx))
                                    {
                                        // Captured by reference: CopyForDeref
                                        // materializes the reference value, not
                                        // the pointee.  Preserve it in the
                                        // separate stack-reference relation.
                                        if !binding.stack_ref_targets.is_empty() {
                                            stack_ref_targets
                                                .entry(dest.clone())
                                                .or_default()
                                                .extend(
                                                    binding
                                                        .stack_ref_targets
                                                        .iter()
                                                        .cloned(),
                                                );
                                        }

                                        // Captured by value: CopyForDeref
                                        // materializes the captured value
                                        // itself, so heap/raw-handle free-flow
                                        // can be connected directly.
                                        for source in &binding.by_value_sources {
                                            let source =
                                                canonical_mir_local(source);
                                            free_flow_parent
                                                .entry(dest.clone())
                                                .or_insert(dest.clone());
                                            free_flow_parent
                                                .entry(source.clone())
                                                .or_insert(source.clone());
                                            union(
                                                &dest,
                                                &source,
                                                &mut free_flow_parent,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        let direct_src = stmt
                            .rvalue
                            .as_deref()
                            .and_then(direct_stack_borrow_source)
                            .or_else(|| {
                                let pos = stmt.details.find('&')?;
                                let tail = &stmt.details[pos..];
                                let end = tail.find(')').unwrap_or(tail.len());
                                direct_stack_borrow_source(&tail[..end])
                            });

                        if let Some(src) = direct_src {
                            let src = canonical_mir_local(&src);
                            stack_ref_targets
                                .entry(dest.clone())
                                .or_default()
                                .insert(src);
                        }

                        // Materializing the value through a reference is the
                        // point at which the loaded value may reconnect to a
                        // heap/raw-handle free-flow component.
                        let deref_source = stmt
                            .rvalue
                            .as_deref()
                            .and_then(direct_deref_value_source)
                            .or_else(|| {
                                for marker in ["copy (*", "move (*"] {
                                    if let Some(pos) = stmt.details.find(marker) {
                                        let tail = &stmt.details[pos..];
                                        if let Some(close) = tail.find(')') {
                                            if let Some(source) =
                                                direct_deref_value_source(&tail[..=close])
                                            {
                                                return Some(source);
                                            }
                                        }
                                    }
                                }
                                None
                            });

                        if let Some(reference) = deref_source {
                            let reference = canonical_mir_local(&reference);
                            let targets =
                                resolve_stack_ref_values(&reference, &stack_ref_targets);

                            for target in targets {
                                let target = canonical_mir_local(&target);

                                // If the referenced stack value is itself a
                                // reference, one dereference materializes that
                                // reference value rather than its pointee.
                                if let Some(nested_targets) =
                                    stack_ref_targets.get(&target).cloned()
                                {
                                    stack_ref_targets
                                        .entry(dest.clone())
                                        .or_default()
                                        .extend(nested_targets);
                                } else {
                                    free_flow_parent
                                        .entry(dest.clone())
                                        .or_insert(dest.clone());
                                    free_flow_parent
                                        .entry(target.clone())
                                        .or_insert(target.clone());
                                    union(&dest, &target, &mut free_flow_parent);
                                }
                            }
                        }
                    }

                    // USE: search pattern "&(*"
                    if stmt.details.contains("&(*") {
                        if let Some(start_idx) = stmt.details.find("&(*") {
                            if let Some(end_idx) = stmt.details[start_idx..].find(")") {
                                let var_used = stmt.details[start_idx + 3..start_idx + end_idx].trim().to_string();
                                let var_used = normalize_name(&var_used);
                               // println!("Found use pattern: '{}' -> '{}'", stmt.details, var_used);
                                if let Some(info) = var_info.get_mut(&var_used) {
                                    info.used = true;
                                    info.use_nodes.insert(node_id.clone());
                                    if info.use_span.is_none() {
                                        info.use_span = Some(stmt.source_info.span.clone());
                                   //     println!("Marking '{}' as used at span: {}", var_used, stmt.source_info.span);
                                    }
                                } else {
                                    let alt = format!("Local({})", var_used);
                                    let alt = normalize_name(&alt);
                                    if let Some(info) = var_info.get_mut(&alt) {
                                        info.used = true;
                                        info.use_nodes.insert(node_id.clone());
                                        if info.use_span.is_none() {
                                            info.use_span = Some(stmt.source_info.span.clone());
                                        //    println!("Marking alternative '{}' as used at span: {}", alt, stmt.source_info.span);
                                        }
                                    } else {
                                   //     println!("No tracked variable found for '{}' or '{}'", var_used, alt);
                                    }
                                }
                            }
                        }

                    // DETECT USE LIKE: "Assign((_12, copy (*_9)))"
                    } else if stmt.details.contains("copy (*"){
                        // xtract variable that is being copied
                        if let Some(start_idx) = stmt.details.find("copy (*") {
                            if let Some(end_idx) = stmt.details[start_idx..].find(")") {
                                let var_used = stmt.details[start_idx + "copy (*".len() .. start_idx + end_idx]
                                    .trim()
                                    .to_string();
                                let var_used_norm = normalize_name(&var_used);
                               // println!("Found use pattern (copy): '{}' -> '{}'", stmt.details, var_used_norm);

                                // first try direct lookup:
                                if let Some(info) = var_info.get_mut(&var_used_norm) {
                                    info.used = true;
                                    info.use_nodes.insert(node_id.clone());
                                    if info.use_span.is_none() {
                                        info.use_span = Some(stmt.source_info.span.clone());
                                       // println!("Marking '{}' as used at span: {}", var_used_norm, stmt.source_info.span);
                                    }
                                } else {
                                    // try with the "Local(...)" wrapper
                                    let alt = normalize_name(&format!("Local({})", var_used_norm));
                                    if let Some(info) = var_info.get_mut(&alt) {
                                        info.used = true;
                                        info.use_nodes.insert(node_id.clone());
                                        if info.use_span.is_none() {
                                            info.use_span = Some(stmt.source_info.span.clone());
                                          //  println!("Marking alternative '{}' as used at span: {}", alt, stmt.source_info.span);
                                        }
                                    } else {
                                        // FALLBACK: use the union-find free flow mapping
                                        let rep = find(&alt, &mut free_flow_parent);
                                        if rep != alt {
                                            if let Some(info) = var_info.get_mut(&rep) {
                                                info.used = true;
                                                info.use_nodes.insert(node_id.clone());
                                                if info.use_span.is_none() {
                                                    info.use_span = Some(stmt.source_info.span.clone());
                                                }
                                          //      println!("Marking union-find representative '{}' as used (derived from '{}')", rep, alt);
                                            } else {
                                           //     println!("No tracked variable found for union-find rep '{}' (derived from '{}')", rep, alt);
                                            }
                                        } else {
                                         //   println!("No tracked variable found for '{}' or '{}'", var_used_norm, alt);
                                        }
                                    }
                                }
                            }
                        }
                          
                    } else if stmt.details.contains("copy ") && stmt.details.contains(" as") {
                        // 1) pull out the var name before the " as"
                        if let Some(start) = stmt.details.find("copy ") {
                            let start = start + "copy ".len();
                            if let Some(end) = stmt.details[start..].find(" as") {
                                let var_used = stmt.details[start..start + end].trim().to_string();
                                // normalize to map-key 
                                let nm = normalize_name(&var_used);
                                // also build the MIR-style wrapper:
                                let wrapped = normalize_name(&format!("Local({})", nm));
                                //  println!("Found use pattern (as-cast): '{}' -> '{}'", stmt.details, nm);
                    
                                // 2) build list of candidates: raw, wrapped, e i loro UF reps
                                let mut cands = Vec::new();
                                cands.push(nm.clone());
                                cands.push(wrapped.clone());
                    
                                // now union-find for each candidate
                                let rep1 = find(&nm, &mut free_flow_parent);
                                let rep2 = find(&wrapped, &mut free_flow_parent);
                                cands.push(rep1.clone());
                                cands.push(rep2.clone());
                    
                                // 3) try
                                let mut marked = false;
                                for key in cands {
                                    if let Some(info) = var_info.get_mut(&key) {
                                        info.used = true;
                                        info.use_nodes.insert(node_id.clone());
                                        if info.use_span.is_none() {
                                            info.use_span = Some(stmt.source_info.span.clone());
                                        }
                                      //  println!("* Marking '{}' (rep of {}) used at {}", key, nm, stmt.source_info.span);
                                        marked = true;
                                        break;
                                    }
                                }
                                if !marked {
                                 //   println!("é No tracked variable found for '{}' (nor wrapped o rep)", nm);
                                }
                            }
                        } 
                    }

                }

                if let Some(MirTerminator::Drop { details, source_info, .. }) = &mir_block.terminator {
                    if details.contains("drop(") {
                        // Preserve the existing unwind policy: a Drop block
                        // reached through an unwind edge is not counted as a
                        // normal-path deallocation.
                        let skip_drop = !panic_unwind_lifecycle_v1_enabled()
                            && icfg.icfg_edges.iter().any(|e| {
                                e.destination == node_id
                                    && matches!(
                                        e.label.as_deref(),
                                        Some(label) if label.contains("unwind")
                                    )
                            });

                        if skip_drop {
                            println!(
                                "-> Skip Drop in '{}' because has an unwind incoming edge",
                                node_id
                            );
                        } else if let Some(start_idx) = details.find("drop(") {
                            if let Some(end_idx) = details[start_idx..].find(')') {
                                let var_dropped =
                                    details[start_idx + 5..start_idx + end_idx]
                                        .trim()
                                        .to_string();
                                let var_dropped = normalize_name(&var_dropped);

                                println!(
                                    "Found drop pattern: '{}' -> '{}'",
                                    details, var_dropped
                                );

                                pending_mir_drops.push((
                                    var_dropped,
                                    source_info.clone(),
                                ));
                            }
                        }
                    }
                }


                if let Some(MirTerminator::Call {details, source_info, function_called, arguments, return_place, allocation_disposition_evidence, ..}) = &mir_block.terminator {
                    if let Some(reason) =
                        c_malloc_rust_allocator_contract_warning(function_called)
                    {
                        if let Some(first_arg) = arguments.first() {
                            let source = canonical_mir_local(&first_arg.arg);
                            let block_has_origin = taint_states
                                .get(&node_id)
                                .and_then(|m| m.get(&source))
                                .map(taint_has_c_malloc_origin)
                                .unwrap_or(false);

                            if block_has_origin
                                || c_malloc_origin_vars.contains(&source)
                            {
                                ffi_allocator_warnings.push((
                                    source,
                                    source_info.clone(),
                                    reason.to_string(),
                                ));
                            }
                        }
                    }

                    // new alias case: multiple allocation on CString::from_raw
                    if is_owning_from_raw_call(function_called)
                        || FROM_RAW_REGEX.is_match(&function_called)
                        || BOX_VEC_FROM_RAW_REGEX.is_match(&function_called)
                        || VEC_FROM_RAW_REGEX.is_match(&function_called)
                        || BOX_FROM_RAW_NODE_GENERIC_REGEX.is_match(&function_called)
                        || is_vec_from_raw_parts_call(function_called)
                        || is_string_from_raw_parts_call(function_called)
                    {
                        /* get the mir names:
                           - src is the local having ptr, e.g. "_13"
                           - dst is new CString local, e.g. "_47" */
                        let src = normalize_name(&arguments[0].arg);
                        let src_canonical = canonical_mir_local(&src);
                        let dst = normalize_name(return_place);
                        // ENSURE both have VarInfo
                        var_info.entry(src.clone()).or_insert_with(VarInfo::new);
                        var_info.entry(dst.clone()).or_insert_with(VarInfo::new);
                        
                        // COPY existing info from src -> dst (keep src).
                        // Fall back to canonical spelling used by stack-ref flow.
                        if let Some(info) = var_info
                            .get(&src)
                            .cloned()
                            .or_else(|| var_info.get(&src_canonical).cloned())
                        {
                            var_info.insert(dst.clone(), info);
                        }
                        // UNION in free-flow: set union‐find sets for Name, then union 
                        free_flow_parent.entry(src.clone()).or_insert(src.clone());
                        free_flow_parent
                            .entry(src_canonical.clone())
                            .or_insert(src_canonical.clone());
                        free_flow_parent.entry(dst.clone()).or_insert(dst.clone());

                        union(&src, &src_canonical, &mut free_flow_parent);
                        union(&src_canonical, &dst, &mut free_flow_parent);

                        // handle also “Local(_47)”
                        let dst_local = format!("Local({})", dst);
                        let dst_local = normalize_name(&dst_local);
                        var_info.entry(dst_local.clone()).or_insert_with(VarInfo::new);
                        free_flow_parent.entry(dst_local.clone()).or_insert(dst_local.clone());
                        union(&dst, &dst_local, &mut free_flow_parent);
                        println!("Aliased via from_raw: '{}' -> '{}'", src, dst);
                    }

                    // Direct allocator deallocation is an explicit free event.
                    if is_raw_dealloc_call(function_called) {
                        if let Some(arg) = arguments.get(0) {
                            pending_std_deallocs.push((
                                normalize_name(&arg.arg),
                                source_info.clone(),
                            ));
                        }
                    }

                    // A C-family free can be called directly from Rust.  The
                    // call site is MIR, but allocator compatibility is defined
                    // by the malloc/free family rather than by source language.
                    if is_c_free_function(function_called, &ffi_functions) {
                        if let Some(arg) = arguments.get(0) {
                            pending_c_free_calls.push((
                                canonical_mir_local(&arg.arg),
                                source_info.clone(),
                                node_id.clone(),
                            ));
                        }
                    }

                    // Pointer operations below require the pointer to denote a
                    // live/valid object. Record a deferred "use" so that alias
                    // information discovered later in traversal can resolve it.
                    if is_pointer_memory_use_call(function_called)
                        || is_cstr_from_ptr_call(function_called)
                    {
                        if let Some(arg) = arguments.get(0) {
                            pending_pointer_uses.push((
                                normalize_name(&arg.arg),
                                source_info.clone(),
                                node_id.clone(),
                            ));
                        }
                    }

                    // EXPLICIT std::mem::drop FUNCTION CALL
                    // Use exactly the same semantics as the transfer function:
                    //   drop(*mut T / *const T) -> no heap free
                    //   drop(tracked owning value) -> one Drop free
                    let certified_raw_pointer_drop = allocation_disposition_evidence
                        .as_ref()
                        .is_some_and(|e| matches!(
                            e.kind,
                            RustAllocationDispositionEvidenceKind::MemDropRawPointer
                        ));
                    if is_explicit_mem_drop(function_called)
                        && !is_raw_pointer_mem_drop(function_called)
                        && !certified_raw_pointer_drop
                    {
                        if let Some(arg_struct) = arguments.get(0) {
                            let dropped_name = extract_arg_name(&arg_struct.arg);
                            let var_dropped = full_local_name(&dropped_name);

                            // Count the drop only when this value participates in
                            // CREMA's tracked heap state (or is already tracked by
                            // the detector). This avoids manufacturing frees for
                            // unrelated scalar/non-heap values.
                            let tracked_heap_value =
                                abs_state.get_allocation(&var_dropped).is_some()
                                || var_info.contains_key(&var_dropped);

                            if tracked_heap_value {
                                if let Some(info) = var_info.get_mut(&var_dropped) {
                                    if info.drop_free == 0 {
                                        info.drop_free = 1;
                                        info.free_span =
                                            Some((source_info.clone(), FreeKind::Drop));
                                    } else {
                                        info.drop_free += 1;
                                    }
                                } else {
                                    var_info.insert(
                                        var_dropped.clone(),
                                        VarInfo {
                                            llvm_free: 0,
                                            drop_free: 1,
                                            c_free_mir: 0,
                                            used: false,
                                            use_span: None,
                                            free_span: Some((
                                                source_info.clone(),
                                                FreeKind::Drop,
                                            )),
                                            use_nodes: BTreeSet::new(),
                                            llvm_free_nodes: BTreeSet::new(),
                                            c_free_mir_nodes: BTreeSet::new(),
                                        },
                                    );
                                }
                            }
                        }
                    }


                }
            },
            GlobalICFGNode::DummyCall(dummy_call) => {
               // println!("-> DummyCall node '{}': MIR var {:?} -> LLVM var {:?}", node_id, dummy_call.mir_var, dummy_call.llvm_var);
                if let (Some(mir_var), Some(llvm_var)) = (&dummy_call.mir_var, &dummy_call.llvm_var) {
                    let mir_var = normalize_name(mir_var);
                    let llvm_var = normalize_name(llvm_var);
            
                    svf_to_name.insert(llvm_var.clone(), llvm_var.clone());
            
                    if !dummy_call.is_internal.unwrap_or(false) {
                        if var_info.contains_key(&mir_var) {
                            // Passing an argument to C is non-consuming.  Do
                            // not move/copy historical event counters into the
                            // formal VarInfo: that would duplicate prior frees
                            // when the same pointer is passed more than once.
                            // Instead create an empty event record for the LLVM
                            // alias and relate both names in free-flow.
                            var_info
                                .entry(llvm_var.clone())
                                .or_insert_with(VarInfo::new);
                            free_flow_parent
                                .entry(llvm_var.clone())
                                .or_insert(llvm_var.clone());
                            free_flow_parent
                                .entry(mir_var.clone())
                                .or_insert(mir_var.clone());
                            union(&mir_var, &llvm_var, &mut free_flow_parent);
                        } else {
                            println!("No tracking info per MIR var '{}'", mir_var);
                        }
                    } else {
                        if let Some(info) = var_info.remove(&mir_var) {
                            println!("Transferring from '{}' to '{}'", mir_var, llvm_var);
                            var_info.insert(llvm_var.clone(), info);
                        } else {
                            println!("No tracking info per MIR var '{}'", mir_var);
                        }
                    }
                }
            },
            GlobalICFGNode::Llvm(llvm_node) => {
                for result in llvm_c_allocation_result_vars(&llvm_node) {
                    let key = scoped_llvm_var(result, &node_id);
                    svf_to_name.entry(key.clone()).or_insert(key);
                }

                for stmt in &llvm_node.svf_statements {
                    let lhs_key = stmt
                        .result_var_id()
                        .map(|id| scoped_llvm_var(id, &node_id));

                    let source_names: Vec<Name> =
                        llvm_provenance_flow_sources(stmt)
                            .into_iter()
                            .map(|id| scoped_llvm_var(id, &node_id))
                            .map(|key| {
                                svf_to_name.get(&key).cloned().unwrap_or(key)
                            })
                            .collect();

                    if let Some(lhs_key) = lhs_key {
                        if let Some(source) = preferred_provenance_source(
                            &source_names,
                            &c_malloc_origin_vars,
                        ) {
                            svf_to_name.insert(lhs_key.clone(), source);
                        } else {
                            svf_to_name
                                .entry(lhs_key.clone())
                                .or_insert(lhs_key.clone());
                        }

                        if let Some(eq_pos) = stmt.stmt_info.find('=') {
                            let before_eq = &stmt.stmt_info[..eq_pos];
                            if let Some(percent_pos) = before_eq.rfind('%') {
                                let digits: String =
                                    before_eq[percent_pos + 1..]
                                        .chars()
                                        .take_while(|c| c.is_ascii_digit())
                                        .collect();

                                if let Ok(ir_id) = digits.parse::<usize>() {
                                    let logical = svf_to_name
                                        .get(&lhs_key)
                                        .cloned()
                                        .unwrap_or(lhs_key.clone());
                                    ir_to_name.insert(
                                        scoped_llvm_ir_var(ir_id, &node_id),
                                        logical,
                                    );
                                }
                            }
                        }
                    }
                }

                if llvm_node.info.contains("@free(")
                    && llvm_node.node_kind_string == "FunCallBlock"
                {
                    if let Some(ir_id) =
                        llvm_free_ir_argument_id(&llvm_node.info)
                    {
                        pending_llvm_free_calls.push((
                            scoped_llvm_ir_var(ir_id, &node_id),
                            llvm_node.info.clone(),
                            node_id.clone(),
                        ));
                    }
                }
            },
            GlobalICFGNode::DummyRet(dummy_ret) => {
                println!(
                    "-> DummyRet node '{}': transferring from LLVM var {:?} to MIR var {:?}",
                    node_id, dummy_ret.llvm_var, dummy_ret.mir_var
                );

                if let (Some(mir_var), Some(llvm_var)) =
                    (&dummy_ret.mir_var, &dummy_ret.llvm_var)
                {
                    let mir_var = canonical_mir_local(mir_var);
                    let llvm_var = normalize_name(llvm_var);
                    let logical_llvm = svf_to_name
                        .get(&llvm_var)
                        .cloned()
                        .unwrap_or_else(|| llvm_var.clone());

                    if let Some(info) = var_info
                        .remove(&logical_llvm)
                        .or_else(|| var_info.remove(&llvm_var))
                    {
                        var_info.insert(mir_var.clone(), info);

                        free_flow_parent
                            .entry(logical_llvm.clone())
                            .or_insert(logical_llvm.clone());
                        free_flow_parent
                            .entry(mir_var.clone())
                            .or_insert(mir_var.clone());
                        union(
                            &logical_llvm,
                            &mir_var,
                            &mut free_flow_parent,
                        );

                        if c_malloc_origin_vars.contains(&logical_llvm)
                            || c_malloc_origin_vars.contains(&llvm_var)
                        {
                            c_malloc_origin_vars.insert(mir_var.clone());
                        }
                    }
                }
            },
            GlobalICFGNode::Terminal(_) => {
                // Explicit maximal control-flow state; no memory event is executed here.
            },
        }

        // ENQUEUE successors in increasing order (this fix the fact that sometime the mir mixes up the order of the successors)
        let mut succs: Vec<_> = icfg
            .icfg_edges
            .iter()
            .filter(|e| e.source == node_id)
            .map(|e| (e.destination.clone(), e.label.clone()))
            .collect();

        succs.sort_by(|a, b| a.0.cmp(&b.0));

        for (dest, _label) in succs {
          //  println!("Enqueuing successor '{}' (edge label: {:?})", dest, label);
            if !visited.contains(&dest) {
                worklist.push_back(dest);
            }
        }
    }

  //  println!("Traversal complete. Final per-variable info: {:?}", var_info);

    // ---- FORCE THE UNION: based on abs_state of ALL the vars----
    let all_vars: BTreeSet<_> = var_info
        .keys()
        .chain(free_flow_parent.keys())
        .cloned()
        .collect();

    for var in all_vars.iter() {
        // Reconcile against the complete MAY-alias relation accumulated over
        // every abstract program point.  Using only `get_allocation(var)` here
        // selected one arbitrary HashMap entry and could split a real alias
        // component depending on map iteration order.
        for candidate in abs_state.get_may_aliases(var) {
            let cn = normalize_name(&candidate);
            if all_vars.contains(&cn) {
                free_flow_parent.entry(var.clone()).or_insert(var.clone());
                union(var, &cn, &mut free_flow_parent);
            }
        }
    }

    // ---- CREATION FREE-FLOWS GROUPS ----
    let mut free_flow_groups: BTreeMap<Name, Vec<Name>> = BTreeMap::new();
    //copy the keys to evitare borrow error
    let parent_keys: Vec<Name> = free_flow_parent.keys().cloned().collect();

    for var in parent_keys {
        let rep = find(&var, &mut free_flow_parent);
        free_flow_groups.entry(rep).or_default().push(var.clone());
    }

    let mut free_flow_keys: BTreeMap<Name, String> = BTreeMap::new();
    for (rep, mut group) in free_flow_groups {
        group.sort();
        free_flow_keys.insert(rep.clone(), format!("{{{}}}", group.join(", ")));
    }

    // ---- RESOLVE INLINED LLVM free() AFTER ALIAS DISCOVERY ----
    //
    // This makes C free accounting independent from incidental ordered_nodes
    // order and from whether the MIR<->LLVM formal bridge was visited before
    // the free node.  There is deliberately no "pick any tracked variable"
    // fallback: an unresolved free stays unresolved rather than being assigned
    // to the wrong allocation.
    for (ir_key, free_info, free_node_id) in pending_llvm_free_calls {
        let Some(mapped) = ir_to_name.get(&ir_key).cloned() else {
            continue;
        };

        let Some(key) =
            resolve_drop_tracking_key(&mapped, &var_info, &free_flow_keys)
        else {
            continue;
        };

        let matching_c_family = is_known_c_malloc_family(
            &key,
            &free_flow_keys,
            &c_malloc_origin_vars,
        );

        let processed_key = if matching_c_family {
            format!("{}@@{}", key, free_node_id)
        } else {
            lookup_free_flow_group(&key, &free_flow_keys)
                .unwrap_or_else(|| key.clone())
        };

        if processed_llvm_free.insert(processed_key) {
            if let Some(info) = var_info.get_mut(&key) {
                info.llvm_free += 1;
                if matching_c_family {
                    info.llvm_free_nodes.insert(free_node_id.clone());
                }
                if info.free_span.is_none() {
                    info.free_span = Some((
                        free_info.clone(),
                        if matching_c_family {
                            FreeKind::CMallocFree
                        } else {
                            FreeKind::LLVM
                        },
                    ));
                }

                if !matching_c_family && info.drop_free == 0 {
                    llvm_warnings.push((key.clone(), free_info));
                }
            }
        }
    }

    // ---- RESOLVE DIRECT C free() CALLS AFTER ALIAS DISCOVERY ----
    for (var_freed, source_info, free_node_id) in pending_c_free_calls {
        if let Some(key) =
            resolve_drop_tracking_key(&var_freed, &var_info, &free_flow_keys)
        {
            let matching_c_family = is_known_c_malloc_family(
                &key,
                &free_flow_keys,
                &c_malloc_origin_vars,
            );

            if let Some(info) = var_info.get_mut(&key) {
                info.c_free_mir += 1;
                if matching_c_family {
                    info.c_free_mir_nodes.insert(free_node_id);
                }
                if info.free_span.is_none() {
                    info.free_span = Some((
                        source_info.clone(),
                        FreeKind::CMallocFreeRustCall,
                    ));
                }
            }

            // If CREMA tracks the allocation but has no positive C-malloc
            // provenance for its component, keep the pre-existing Rust->C
            // allocator-mismatch warning.  Untracked/unknown origins do not
            // reach this branch and are not classified.
            if !matching_c_family {
                c_free_mismatch_warnings.push((
                    key,
                    source_info,
                ));
            }
        }
    }

    // ---- RESOLVE DIRECT std::alloc::dealloc AFTER ALIAS DISCOVERY ----
    for (var_freed, source_info) in pending_std_deallocs {
        if let Some(key) =
            resolve_drop_tracking_key(&var_freed, &var_info, &free_flow_keys)
        {
            if let Some(info) = var_info.get_mut(&key) {
                if info.drop_free == 0 {
                    info.drop_free = 1;
                    info.free_span =
                        Some((source_info.clone(), FreeKind::StdDealloc));
                } else {
                    info.drop_free += 1;
                }
            }
        }
    }

    // ---- RESOLVE POINTER/CStr USES AFTER ALIAS DISCOVERY ----
    for (var_used, source_info, use_node_id) in pending_pointer_uses {
        if let Some(key) =
            resolve_drop_tracking_key(&var_used, &var_info, &free_flow_keys)
        {
            if let Some(info) = var_info.get_mut(&key) {
                info.used = true;
                info.use_nodes.insert(use_node_id);
                if info.use_span.is_none() {
                    info.use_span = Some(source_info);
                }
            }
        }
    }

    // ---- RESOLVE NORMAL MIR DROPS AFTER ALIAS DISCOVERY ----
    //
    // This makes free accounting independent from the incidental ICFG traversal
    // order.  In particular a `Box::from_raw` alias discovered later in the
    // traversal can still be matched with an earlier `drop(_N)` event.
    for (var_dropped, source_info) in pending_mir_drops {
        if let Some(key) =
            resolve_drop_tracking_key(&var_dropped, &var_info, &free_flow_keys)
        {
            if let Some(info) = var_info.get_mut(&key) {
                if info.drop_free == 0 {
                    info.drop_free = 1;
                    info.free_span =
                        Some((source_info.clone(), FreeKind::Drop));
                } else {
                    info.drop_free += 1;
                }
            }
        } else {
            let alt = normalize_name(&format!("Local({})", var_dropped));
            println!(
                "No tracked variable found per '{}' o '{}'",
                var_dropped, alt
            );
        }
    }

    // ---- FILTER: retain last visited node variables (of the main) OR having free_count > 0 ----
    if let Some(last_node) = get_last_visited() {
        if let Some(last_mir_main_id) = icfg.ordered_nodes.iter().rev().find_map(|(node_id, _)| {
            if node_id.starts_with(&last_node)
               && !icfg.icfg_edges.iter().any(|e| e.source == *node_id)
            {
                Some(node_id.clone())
            } else {
                None
            }
        }) {
         //   println!("Filtering taint info per last MIR node (from main) '{}'...", last_mir_main_id);
            if let Some(last_block_vars) = taint_states.get(&last_mir_main_id) {
                let last_keys: std::collections::HashSet<_> = last_block_vars.keys().cloned().collect();

                var_info.retain(|k, info| {
                    // determine if the allocation (or its group) is in the last MIR node
                    if last_keys.contains(k) {
                        true
                    } else if let Some(group) = lookup_free_flow_group(k, &free_flow_keys) {
                        group_contains_any(&group, &last_keys)
                            || info.effective_free() > 0
                            || group_contains_any(&group, &c_malloc_origin_vars)
                    } else {
                        info.effective_free() > 0
                            || c_malloc_origin_vars.contains(k)
                    }
                });
            } else {
                println!("NO taint state found for the last mir node '{}'", last_mir_main_id);
            }
        }
    }
        // FINAL MERGE: from var_info -> alloc_info, mantaining use_span e (span, FreeKind)
        let mut alloc_info: HashMap<String,(
            usize,                     // free count
            bool,                      // used ?
            Option<String>,            // use_span
            Option<(String, FreeKind)>, // free_span + kind
            BTreeSet<String>,           // semantic use ICFG nodes
            BTreeSet<String>,           // matching LLVM C-free nodes
            BTreeSet<String>            // matching MIR direct-C-free nodes
        )> = HashMap::new();

        for (var, info) in var_info.into_iter() {
        let fc = info.effective_free();
        let used = info.used;
        let use_span = info.use_span;
        let free_span = info.free_span; // Option<(String, FreeKind)>

        let norm = normalize_name(&var);
        let alloc_key = if let Some(group_key) = lookup_free_flow_group(&norm, &free_flow_keys) {
            group_key.clone()
        } else if let Some(alloc) = abs_state.get_allocation(&norm) {
            format!("{:?}", alloc.set)
        } else {
            norm.clone()
        };

        let entry = alloc_info.entry(alloc_key.clone())
            .or_insert((
                0,
                false,
                None,
                None,
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
            ));
        entry.0 += fc;
        entry.1 = entry.1 || used;
        entry.4.extend(info.use_nodes.iter().cloned());
        entry.5.extend(info.llvm_free_nodes.iter().cloned());
        entry.6.extend(info.c_free_mir_nodes.iter().cloned());

        if entry.2.is_none() {
            entry.2 = use_span.clone();
        }

        if entry.3.is_none() {
            entry.3 = free_span.clone();
        }
        }

        let mut use_after_free: MultiSet = MultiSet::new();
        let mut double_free: MultiSet = MultiSet::new();
        let mut never_free: MultiSet = MultiSet::new();

        for (
            alloc_key,
            (
                fc,
                used,
                use_span_opt,
                free_span_opt,
                use_nodes,
                llvm_free_nodes,
                c_free_mir_nodes,
            ),
        ) in &alloc_info {
            // double-free
            if *fc >= 2 {
                multiset_add(&mut double_free, alloc_key.clone());
            }
            // never-free
            if *fc == 0 {
                multiset_add(&mut never_free, alloc_key.clone());
            }
            // use-after-free / undefined behaviour
            if *used && *fc > 0 {
                match free_span_opt {
                    // case LLVM: ALWAYS report a possible undefined behaviour 
                    // 1) cannot compare span
                    // 2) a memory allocated in Rust and then freed in C may lead to undefined behaviour depending on the library (es.Cstring)
                    // 3) C uses the std allocator, in Rust may use different allocators (es jemalloc) )
                
                    Some((_, FreeKind::LLVM)) => {
                        multiset_add(&mut use_after_free, alloc_key.clone());
                    }
                    Some((_, FreeKind::CMallocFree)) => {
                        // C source spans and Rust source spans are not
                        // comparable.  Use the inlined semantic ICFG instead:
                        // a path from a matching C free node to a use node is a
                        // possible use-after-free execution.
                        if c_malloc_has_use_after_inlined_free(
                            icfg,
                            llvm_free_nodes,
                            use_nodes,
                        ) {
                            multiset_add(&mut use_after_free, alloc_key.clone());
                        }
                    }
                    Some((_, FreeKind::CMallocFreeRustCall)) => {
                        // Both events are represented in the GlobalICFG, so use
                        // graph semantics rather than lexical source-line order.
                        // This avoids false ordering across mutually exclusive
                        // branches and remains conservative for loops.
                        if c_malloc_has_use_after_inlined_free(
                            icfg,
                            c_free_mir_nodes,
                            use_nodes,
                        ) {
                            multiset_add(&mut use_after_free, alloc_key.clone());
                        }
                    }
                    // Historical Rust-owned deallocation behavior is retained
                    // unchanged for Phase-4.4 compatibility.
                    Some((free_span, FreeKind::Drop | FreeKind::StdDealloc)) => {
                        if let Some(use_span) = use_span_opt {
                            if let (Some(use_line), Some(free_line)) =
                                (span_to_line(use_span), span_to_line(free_span))
                            {
                                if use_line > free_line {
                                    multiset_add(&mut use_after_free, alloc_key.clone());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // visit alloc_info with only Option<String> for the "free_span"
        let report_alloc_info: HashMap<String, (usize, bool, Option<String>, Option<String>)> =
            alloc_info
                .into_iter()
                .map(|(k, (fc, used, use_span, free_span_opt, _, _, _))| {
                    let free_span_str = free_span_opt.map(|(span, _kind)| span);
                    (k, (fc, used, use_span, free_span_str))
                })
                .collect();

       
        print_final_report(&report_alloc_info, &use_after_free, &double_free, &never_free);

        // warning per free LLVM
        for (var, span) in llvm_warnings {
            println!(
                "WARNING: variable '{}' was allocated in Rust and then freed in C (LLVM free) at `{}`",
                var, span
            );
            println!("Possible UNDEFINED BEHAVIOUR!");
        }

        for (var, span, reason) in ffi_allocator_warnings {
            println!(
                "WARNING: C malloc-family allocation '{}' reaches a Rust allocator/ownership API whose safety contract is not established at `{}`: {}",
                var, span, reason
            );
            println!("Possible UNDEFINED BEHAVIOUR!");
        }

        for (var, span) in c_free_mismatch_warnings {
            println!(
                "WARNING: tracked allocation '{}' without positive C malloc-family provenance is passed directly to C free() at `{}`",
                var, span
            );
            println!("Possible UNDEFINED BEHAVIOUR!");
        }

        (use_after_free, double_free, never_free)
}


fn group_contains_any(group: &String, last_keys: &HashSet<String>) -> bool {
    // the group string is formatted like "{elem1, elem2, ...}"
    // strip off the curly braces, split by commas, trim each element
    group.trim_matches(|c| c == '{' || c == '}')
         .split(',')
         .any(|s| last_keys.contains(s.trim()))
}


fn print_final_report(alloc_info: &HashMap<String, (usize, bool, Option<String>, Option<String>)>, use_after_free: &MultiSet, double_free: &MultiSet, never_free: &MultiSet) {
    //top border
    for _ in 0..38 {
        print!("\u{2B26}");
    }
    println!();

    // booleans for non-emptiness
    let u = !use_after_free.is_empty();
    let d = !double_free.is_empty();
    let n = !never_free.is_empty();

    match (u, d, n) {
        // only use-after-free 
        (true, false, false) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Use-After-Free Issues / Undefined behaviour \u{2622}:");
            for (_alloc_key, (_fc, _used, use_span, _free_span)) in alloc_info.iter() {
                if let Some(s) = use_span {
                    println!("Use detected at source line: {}", s);
                }
            }
            //println!("{:?}", use_after_free);
        }
        // use-after-free and double free issues, no never free
        (true, true, false) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Use-After-Free Issues / Undefined behaviour \u{2622}:");
            for (_alloc_key, (_fc, _used, use_span, _free_span)) in alloc_info.iter() {
                if let Some(s) = use_span {
                    println!("Use detected at source line: {}", s);
                }
            }
            println!("{:?}", use_after_free);
            println!("\u{2622} Double Free Issues \u{2622}:");
            for (alloc_key, (_fc, _used, _use_span, free_span)) in alloc_info.iter() {
                if double_free.contains_key(alloc_key) {
                    if let Some(s) = free_span {
                        println!("Free detected at source line: {}", s);
                    } else {
                        println!("Free detected (no source span available) for allocation {}", alloc_key);
                    }
                }
            }
        }
        // all issues present
        (true, true, true) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Use-After-Free Issues / Undefined behaviour \u{2622}: {:?}", use_after_free);
            for (_alloc_key, (_fc, _used, use_span, _free_span)) in alloc_info.iter() {
                if let Some(s) = use_span {
                    println!("Use detected at source line: {}", s);
                }
            }
            println!("\u{2622} Double Free Issues \u{2622}:");
            for (alloc_key, (_fc, _used, _use_span, free_span)) in alloc_info.iter() {
                if double_free.contains_key(alloc_key) {
                    if let Some(s) = free_span {
                        println!("Free detected at source line: {}", s);
                    } else {
                        println!("Free detected (no source span available) for allocation {}", alloc_key);
                    }
                }
            }
            println!("\u{2622} Never Free Issues \u{2622}:");
            println!("{:?}", never_free);
        }
        // only double free 
        (false, true, false) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Double Free Issues \u{2622}:");
            for (alloc_key, (_fc, _used, _use_span, free_span)) in alloc_info.iter() {
                if double_free.contains_key(alloc_key) {
                    if let Some(s) = free_span {
                        println!("Free detected at source line: {}", s);
                    } else {
                        println!("Free detected (no source span available) for allocation {}", alloc_key);
                    }
                }
            }
        }
        // double free and never free 
        (false, true, true) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Double Free Issues \u{2622}:");
            for (alloc_key, (_fc, _used, _use_span, free_span)) in alloc_info.iter() {
                if double_free.contains_key(alloc_key) {
                    if let Some(s) = free_span {
                        println!("Free detected at source line: {}", s);
                    } else {
                        println!("Free detected (no source span available) for allocation {}", alloc_key);
                    }
                }
            }
            println!("\u{2622} Never Free Issues \u{2622}:");
            println!("{:?}", never_free);
        }
        // use-after-free and never free 
        (true, false, true) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Use-After-Free Issues / Undefined behaviour \u{2622}:");
            for (_alloc_key, (_fc, _used, use_span, _free_span)) in alloc_info.iter() {
                if let Some(s) = use_span {
                    println!("Use detected at source line: {}", s);
                }
            }
            println!("{:?}", use_after_free);
            println!("\u{2622} Never Free Issues \u{2622}:");
            println!("{:?}", never_free);
        }
        // only never free issues
        (false, false, true) => {
            println!("\u{1F916}\u{1F4AC} Potential memory issues detected \u{1F680}:\n");
            println!("\u{2622} Never Free Issues \u{2622}: {:?}", never_free);
            println!();
        }
        // No issues detected (all sets empty)
        (false, false, false) => {
            println!("\u{1F916}\u{1F4AC} NO Issues detected: \u{2705}\n");
        }
    }

    //bottom border
    for _ in 0..38 {
        print!("\u{2B26}");
    }
    println!();
}

#[cfg(test)]
mod lattice_law_tests {
    use super::CellValue;
    use std::cmp::Ordering;

    const ALL: [CellValue; 8] = [
        CellValue::BOTTOM,
        CellValue::BOXTIMES,
        CellValue::ALLOC,
        CellValue::FREED,
        CellValue::MB,
        CellValue::IMMB,
        CellValue::MV,
        CellValue::TOP,
    ];

    #[test]
    fn boxtimes_has_the_expected_ordering() {
        use CellValue::*;

        assert!(BOTTOM.leq(BOXTIMES));
        assert!(BOXTIMES.leq(TOP));

        for x in [ALLOC, FREED, MB, IMMB, MV] {
            assert!(!BOXTIMES.leq(x), "BOXTIMES must be incomparable with {:?}", x);
            assert!(!x.leq(BOXTIMES), "{:?} must be incomparable with BOXTIMES", x);
            assert_eq!(BOXTIMES.join(x), TOP);
            assert_eq!(BOXTIMES.meet(x), BOTTOM);
        }
    }

    #[test]
    fn partial_order_laws_hold_exhaustively() {
        // Reflexivity.
        for x in ALL {
            assert!(x.leq(x));
        }

        // Antisymmetry.
        for x in ALL {
            for y in ALL {
                if x.leq(y) && y.leq(x) {
                    assert_eq!(x, y);
                }
            }
        }

        // Transitivity.
        for x in ALL {
            for y in ALL {
                for z in ALL {
                    if x.leq(y) && y.leq(z) {
                        assert!(x.leq(z), "{:?} <= {:?} <= {:?} but {:?} !<= {:?}", x, y, z, x, z);
                    }
                }
            }
        }
    }

    #[test]
    fn join_is_a_lub_exhaustively() {
        for x in ALL {
            assert_eq!(x.join(x), x);

            for y in ALL {
                let j = x.join(y);
                assert_eq!(j, y.join(x), "join not commutative for {:?}, {:?}", x, y);
                assert!(x.leq(j), "{:?} is not <= join({:?},{:?})={:?}", x, x, y, j);
                assert!(y.leq(j), "{:?} is not <= join({:?},{:?})={:?}", y, x, y, j);

                for z in ALL {
                    if x.leq(z) && y.leq(z) {
                        assert!(j.leq(z), "join({:?},{:?})={:?} is not least below upper bound {:?}", x, y, j, z);
                    }
                }
            }
        }

        for x in ALL {
            for y in ALL {
                for z in ALL {
                    assert_eq!(
                        x.join(y).join(z),
                        x.join(y.join(z)),
                        "join not associative for {:?}, {:?}, {:?}",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn meet_is_a_glb_exhaustively() {
        for x in ALL {
            assert_eq!(x.meet(x), x);

            for y in ALL {
                let m = x.meet(y);
                assert_eq!(m, y.meet(x), "meet not commutative for {:?}, {:?}", x, y);
                assert!(m.leq(x), "meet({:?},{:?})={:?} is not <= {:?}", x, y, m, x);
                assert!(m.leq(y), "meet({:?},{:?})={:?} is not <= {:?}", x, y, m, y);

                for z in ALL {
                    if z.leq(x) && z.leq(y) {
                        assert!(z.leq(m), "lower bound {:?} is not <= meet({:?},{:?})={:?}", z, x, y, m);
                    }
                }
            }
        }

        for x in ALL {
            for y in ALL {
                for z in ALL {
                    assert_eq!(
                        x.meet(y).meet(z),
                        x.meet(y.meet(z)),
                        "meet not associative for {:?}, {:?}, {:?}",
                        x, y, z
                    );
                }
            }
        }
    }

    #[test]
    fn partial_cmp_matches_leq() {
        for x in ALL {
            for y in ALL {
                let cmp = x.partial_cmp(&y);
                match cmp {
                    Some(Ordering::Equal) => assert_eq!(x, y),
                    Some(Ordering::Less) => assert!(x.leq(y) && !y.leq(x)),
                    Some(Ordering::Greater) => assert!(y.leq(x) && !x.leq(y)),
                    None => assert!(!x.leq(y) && !y.leq(x)),
                }
            }
        }
    }
}

#[cfg(test)]
mod abstract_memory_invariant_tests {
    use super::{AbstractMemory, AbstractState, Allocation, CellValue};
    use std::cmp::Ordering;

    fn n(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn equal_cell_values_do_not_create_aliases() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("x"), CellValue::ALLOC);
        mem.set_cell_value(&n("y"), CellValue::ALLOC);

        assert_ne!(mem.get_allocation(&n("x")), mem.get_allocation(&n("y")));
        assert_eq!(mem.state.len(), 2);
    }

    #[test]
    fn boxtimes_locals_do_not_alias_just_because_values_match() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("x"), CellValue::BOXTIMES);
        mem.set_cell_value(&n("y"), CellValue::BOXTIMES);

        assert_ne!(mem.get_allocation(&n("x")), mem.get_allocation(&n("y")));
        assert_eq!(mem.state.len(), 2);
    }

    #[test]
    fn removing_one_alias_preserves_the_other_alias_state() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("x"), CellValue::ALLOC);
        mem.propagate_cell_value(&n("x"), &n("y"));

        mem.set_cell_value(&n("x"), CellValue::BOTTOM);

        assert_eq!(mem.get_cell_value(&n("x")), CellValue::BOTTOM);
        assert_eq!(mem.get_cell_value(&n("y")), CellValue::ALLOC);
    }

    #[test]
    fn union_computes_alias_equivalence_closure() {
        let mut left = AbstractMemory::default();
        left.set_cell_value(&n("a"), CellValue::ALLOC);
        left.propagate_cell_value(&n("a"), &n("b"));

        let mut right = AbstractMemory::default();
        right.set_cell_value(&n("b"), CellValue::ALLOC);
        right.propagate_cell_value(&n("b"), &n("c"));

        let joined = left.union(&right);
        let expected: std::collections::BTreeSet<_> =
            ["a", "b", "c"].into_iter().map(n).collect();

        assert_eq!(joined.state.len(), 1);
        let (alloc, value) = joined.state.iter().next().unwrap();
        assert_eq!(alloc.set, expected);
        assert_eq!(*value, CellValue::ALLOC);
    }

    #[test]
    fn union_joins_conflicting_states_over_an_alias_component() {
        let mut left = AbstractMemory::default();
        left.set_cell_value(&n("a"), CellValue::ALLOC);
        left.propagate_cell_value(&n("a"), &n("b"));

        let mut right = AbstractMemory::default();
        right.set_cell_value(&n("b"), CellValue::FREED);
        right.propagate_cell_value(&n("b"), &n("c"));

        let joined = left.union(&right);
        let alloc = joined.get_allocation(&n("b")).unwrap();

        assert!(alloc.set.contains("a"));
        assert!(alloc.set.contains("b"));
        assert!(alloc.set.contains("c"));
        assert_eq!(joined.get_cell_value(&n("a")), CellValue::TOP);
        assert_eq!(joined.get_cell_value(&n("b")), CellValue::TOP);
        assert_eq!(joined.get_cell_value(&n("c")), CellValue::TOP);
    }

    #[test]
    fn propagation_detaches_only_destination_from_its_old_aliases() {
        let mut mem = AbstractMemory::default();

        mem.set_cell_value(&n("x"), CellValue::ALLOC);

        mem.set_cell_value(&n("y"), CellValue::FREED);
        mem.propagate_cell_value(&n("y"), &n("z"));
        assert_eq!(mem.get_allocation(&n("y")), mem.get_allocation(&n("z")));

        // y := x. y must move to x's alias component, while z remains on
        // the old FREED allocation.
        mem.propagate_cell_value(&n("x"), &n("y"));

        assert_eq!(mem.get_allocation(&n("x")), mem.get_allocation(&n("y")));
        assert_ne!(mem.get_allocation(&n("y")), mem.get_allocation(&n("z")));
        assert_eq!(mem.get_cell_value(&n("x")), CellValue::ALLOC);
        assert_eq!(mem.get_cell_value(&n("y")), CellValue::ALLOC);
        assert_eq!(mem.get_cell_value(&n("z")), CellValue::FREED);
    }


    #[test]
    fn abstract_memory_partial_cmp_reports_greater() {
        let mut precise = AbstractMemory::default();
        precise.set_cell_value(&n("x"), CellValue::ALLOC);

        let mut less_precise = AbstractMemory::default();
        less_precise.set_cell_value(&n("x"), CellValue::MB);

        assert_eq!(precise.partial_cmp(&less_precise), Some(Ordering::Less));
        assert_eq!(less_precise.partial_cmp(&precise), Some(Ordering::Greater));
    }

    #[test]
    fn more_may_alias_information_is_less_precise() {
        let mut no_alias = AbstractMemory::default();
        no_alias.set_cell_value(&n("x"), CellValue::ALLOC);
        no_alias.set_cell_value(&n("y"), CellValue::ALLOC);

        let mut may_alias = AbstractMemory::default();
        may_alias.set_cell_value(&n("x"), CellValue::ALLOC);
        may_alias.propagate_cell_value(&n("x"), &n("y"));

        assert!(no_alias.leq(&may_alias));
        assert!(!may_alias.leq(&no_alias));
    }

    #[test]
    fn abstract_state_may_aliases_union_all_program_points_independent_of_insertion_order() {
        let mut short = AbstractMemory::default();
        short.set_cell_value(&n("p"), CellValue::MV);
        short.propagate_cell_value(&n("p"), &n("q"));

        let mut long = AbstractMemory::default();
        long.set_cell_value(&n("p"), CellValue::MV);
        long.propagate_cell_value(&n("p"), &n("q"));
        long.propagate_cell_value(&n("p"), &n("r"));

        let mut forward = AbstractState::default();
        forward.insert("bb_short".to_string(), short.clone());
        forward.insert("bb_long".to_string(), long.clone());

        let mut reverse = AbstractState::default();
        reverse.insert("bb_long".to_string(), long);
        reverse.insert("bb_short".to_string(), short);

        let expected: std::collections::BTreeSet<_> =
            ["p", "q", "r"].into_iter().map(n).collect();

        assert_eq!(forward.get_may_aliases(&n("p")), expected);
        assert_eq!(reverse.get_may_aliases(&n("p")), expected);
    }
}

#[cfg(test)]
mod boxtimes_transfer_tests {
    use super::{
        apply_mir_statement, eval_rvalue, AbstractMemory, CellValue, TaintStateMap,
    };
    use crate::structs::{MirStatement, SourceInfoData};

    fn n(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn primitive_scalar_constants_are_boxtimes() {
        let mem = AbstractMemory::default();

        for rvalue in [
            "const 42_i32",
            "const -7_i128",
            "const 1_usize",
            "const true",
            "const false",
            "const 3.5_f64",
            "const 'x'",
            "const ()",
        ] {
            assert_eq!(
                eval_rvalue(rvalue, &mem),
                CellValue::BOXTIMES,
                "expected BOXTIMES for {rvalue}"
            );
        }
    }

    #[test]
    fn promoted_string_and_function_constants_are_not_boxtimes() {
        let mem = AbstractMemory::default();

        for rvalue in [
            "const main::promoted[0]",
            "const SOME_FUNCTION",
            "const \"hello\"",
        ] {
            assert_ne!(
                eval_rvalue(rvalue, &mem),
                CellValue::BOXTIMES,
                "must not classify {rvalue} as BOXTIMES"
            );
        }
    }

    #[test]
    fn scalar_nullary_operations_are_boxtimes() {
        let mem = AbstractMemory::default();

        for rvalue in [
            "SizeOf([i32; 2])",
            "AlignOf(i32)",
            "Len(_3)",
            "Discriminant(_4)",
        ] {
            assert_eq!(eval_rvalue(rvalue, &mem), CellValue::BOXTIMES);
        }
    }

    #[test]
    fn scalar_cast_is_boxtimes_but_pointer_cast_is_not() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        assert_eq!(
            eval_rvalue("copy _1 as usize (Transmute)", &mem),
            CellValue::BOXTIMES
        );

        assert_ne!(
            eval_rvalue("copy _1 as *mut i32 (PtrToPtr)", &mem),
            CellValue::BOXTIMES
        );
    }

    #[test]
    fn direct_scalar_copy_reads_source_value() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::BOXTIMES);

        assert_eq!(eval_rvalue("copy _1", &mem), CellValue::BOXTIMES);
    }

    #[test]
    fn binary_scalar_operations_follow_formal_rule() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::BOXTIMES);
        mem.set_cell_value(&n("Local(_2)"), CellValue::BOXTIMES);

        for rvalue in [
            "Add(copy _1, copy _2)",
            "Sub(copy _1, const 1_i32)",
            "BitAnd(copy _1, copy _2)",
            "Eq(copy _1, const 0_i32)",
        ] {
            assert_eq!(
                eval_rvalue(rvalue, &mem),
                CellValue::BOXTIMES,
                "expected scalar result for {rvalue}"
            );
        }
    }

    #[test]
    fn binary_rule_propagates_top_and_rejects_non_scalar_mix() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::TOP);
        mem.set_cell_value(&n("Local(_2)"), CellValue::BOXTIMES);
        mem.set_cell_value(&n("Local(_3)"), CellValue::ALLOC);

        assert_eq!(
            eval_rvalue("Add(copy _1, copy _2)", &mem),
            CellValue::TOP
        );

        assert_eq!(
            eval_rvalue("Add(copy _2, copy _3)", &mem),
            CellValue::BOTTOM
        );
    }

    fn assign_stmt(place: &str, rvalue: &str) -> MirStatement {
        MirStatement {
            source_info: SourceInfoData {
                span: "<test>".to_string(),
                scope: "<test>".to_string(),
            },
            kind: "Assign".to_string(),
            details: format!("Assign(({}, {}))", place, rvalue),
            place: Some(place.to_string()),
            is_mutable: Some(true),
            rvalue: Some(rvalue.to_string()),
        }
    }

    #[test]
    fn scalar_assignment_writes_boxtimes_without_creating_value_based_aliases() {
        let mem = AbstractMemory::default();
        let mut taint = TaintStateMap::default();

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_1)", "const 42_i32"),
        );
        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "const 7_i32"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::BOXTIMES);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::BOXTIMES);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn scalar_overwrite_detaches_only_destination_from_old_heap_aliases() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);
        mem.propagate_cell_value(&n("Local(_1)"), &n("Local(_2)"));

        let mut taint = TaintStateMap::default();
        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_1)", "const 5_i32"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::BOXTIMES);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::ALLOC);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn borrow_of_scalar_or_untracked_local_is_not_manufactured_as_heap_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::BOXTIMES);

        let mut taint = TaintStateMap::default();
        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "&_1"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::TOP);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );

        let mem2 = AbstractMemory::default();
        let mut taint2 = TaintStateMap::default();
        let mem2 = apply_mir_statement(
            &mem2,
            &mut taint2,
            &assign_stmt("Local(_4)", "&_3"),
        );

        assert_eq!(mem2.get_cell_value(&n("Local(_4)")), CellValue::TOP);
    }


    #[test]
    fn unary_scalar_operations_are_boxtimes() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::BOXTIMES);

        assert_eq!(eval_rvalue("Neg(copy _1)", &mem), CellValue::BOXTIMES);
        assert_eq!(eval_rvalue("Not(copy _1)", &mem), CellValue::BOXTIMES);
    }
}

#[cfg(test)]
mod phase3_1_stabilization_tests {
    use super::{
        join_taint_maps, resolve_drop_tracking_key, AbstractMemory, CellValue,
        FreeKind, TaintStateMap, VarInfo,
    };
    use std::collections::BTreeMap;

    fn n(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn taint_join_preserves_raw_allocation_provenance_when_value_joins_to_top() {
        let mut old_mem = AbstractMemory::default();
        old_mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let mut scalar_mem = AbstractMemory::default();
        scalar_mem.set_cell_value(&n("Local(_1)"), CellValue::BOXTIMES);

        let joined_mem = old_mem.union(&scalar_mem);
        assert_eq!(
            joined_mem.get_cell_value(&n("Local(_1)")),
            CellValue::TOP
        );

        let mut old_taint = TaintStateMap::default();
        old_taint
            .entry(n("Local(_1)"))
            .or_default()
            .insert("assign".to_string());

        let new_taint = TaintStateMap::default();
        let joined_taint =
            join_taint_maps(&old_taint, &new_taint, &joined_mem);

        assert!(
            joined_taint
                .get("Local(_1)")
                .is_some_and(|tags| tags.contains("assign")),
            "MV provenance must survive MV ⊔ BOXTIMES = TOP"
        );
    }

    #[test]
    fn taint_join_is_union_and_never_discards_existing_markers() {
        let mem = AbstractMemory::default();

        let mut left = TaintStateMap::default();
        left.entry(n("x"))
            .or_default()
            .insert("assign".to_string());

        let mut right = TaintStateMap::default();
        right.entry(n("x"))
            .or_default()
            .insert("use".to_string());
        right.entry(n("y"))
            .or_default()
            .insert("free".to_string());

        let joined = join_taint_maps(&left, &right, &mem);

        assert!(joined["x"].contains("assign"));
        assert!(joined["x"].contains("use"));
        assert!(joined["y"].contains("free"));
    }

    #[test]
    fn deferred_drop_resolves_direct_from_raw_destination() {
        let mut info = BTreeMap::new();
        info.insert(n("_8"), VarInfo::new());

        let groups = BTreeMap::new();

        assert_eq!(
            resolve_drop_tracking_key(&n("_8"), &info, &groups),
            Some(n("_8"))
        );
    }

    #[test]
    fn deferred_drop_resolves_via_completed_alias_group() {
        let mut info = BTreeMap::new();
        info.insert(n("Local(_9) [mutable]"), VarInfo::new());

        let mut groups = BTreeMap::new();
        groups.insert(
            n("rep"),
            n("{Local(_9) [mutable], _8, Local(_8)}"),
        );

        assert_eq!(
            resolve_drop_tracking_key(&n("_8"), &info, &groups),
            Some(n("Local(_9) [mutable]"))
        );
    }

    #[test]
    fn counted_deferred_drop_is_one_normal_free() {
        let mut info = VarInfo::new();
        info.drop_free = 1;
        info.free_span =
            Some(("<test>".to_string(), FreeKind::Drop));

        assert_eq!(info.effective_free(), 1);
    }
}


#[cfg(test)]
mod phase5_c_origin_ffi_tests {
    use super::{
        c_malloc_rust_allocator_contract_warning, llvm_call_suffix_from_global_node_id,
        scoped_llvm_var, transfer_dummyret_node, transfer_llvm_node,
        AbstractMemory, CellValue, DummyNode, LlvmJsonNode, SvfStatement,
        TaintStateMap, TAINT_ASSIGN, TAINT_C_MALLOC_FAMILY,
        is_c_free_function,
        is_c_free_call_text,
        VarInfo,
        is_known_c_malloc_family,
        is_c_malloc_family_alloc_call,
        c_malloc_has_use_after_inlined_free,
        icfg_may_reach,
        llvm_provenance_flow_sources,
        preferred_provenance_source,
        detector_pointer_cast_participants
    };
    use std::collections::{BTreeMap, BTreeSet, HashSet};
    use crate::structs::{GlobalICFGOrdered, IcfgEdge, MirStatement, SourceInfoData};

    fn stmt(
        stmt_type: &str,
        stmt_info: &str,
        lhs: Option<usize>,
        rhs: Option<usize>,
        operands: Option<Vec<usize>>,
    ) -> SvfStatement {
        SvfStatement {
            stmt_id: 1,
            stmt_type: stmt_type.to_string(),
            stmt_info: stmt_info.to_string(),
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

    fn assign_stmt(place: &str, rvalue: &str) -> MirStatement {
        MirStatement {
            source_info: SourceInfoData {
                span: "<phase5-test>".to_string(),
                scope: "<phase5-test>".to_string(),
            },
            kind: "Assign".to_string(),
            details: format!("Assign(({}, {}))", place, rvalue),
            place: Some(place.to_string()),
            is_mutable: Some(true),
            rvalue: Some(rvalue.to_string()),
        }
    }

    fn llvm_node(
        kind: &str,
        info: &str,
        statements: Vec<SvfStatement>,
    ) -> LlvmJsonNode {
        LlvmJsonNode {
            node_id: 12,
            node_type: false,
            info: info.to_string(),
            node_kind_string: kind.to_string(),
            node_kind: 0,
            node_source_loc: String::new(),
            function_name: Some("c_alloc".to_string()),
            basic_block: None,
            basic_block_name: None,
            basic_block_info: None,
            svf_statements: statements,
            incoming_edges: Vec::new(),
            outgoing_edges: Vec::new(),
        }
    }

    #[test]
    fn llvm_var_ids_are_scoped_per_replicated_ffi_callsite() {
        let node_id = "llvm::c_alloc::node12::rust::main::bb4";
        assert_eq!(
            llvm_call_suffix_from_global_node_id(node_id),
            Some("rust::main::bb4")
        );
        assert_eq!(
            scoped_llvm_var(20, node_id),
            "20@rust::main::bb4"
        );
    }

    #[test]
    fn malloc_result_is_nullable_top_with_positive_c_family_provenance() {
        let node_id = "llvm::c_alloc::node12::rust::main::bb4";
        let node = llvm_node(
            "FunCallBlock",
            "CallICFGNode12 {fun: c_alloc}\n%call = call noalias ptr @malloc(i64 %n)",
            vec![stmt(
                "AddrStmt",
                "AddrStmt: [Var20 <-- Var21]",
                Some(20),
                Some(21),
                None,
            )],
        );

        let (mem, taint) = transfer_llvm_node(
            node_id,
            &node,
            &AbstractMemory::default(),
            &TaintStateMap::default(),
        );

        assert_eq!(
            mem.get_cell_value(&"20@rust::main::bb4".to_string()),
            CellValue::TOP
        );
        let tags = &taint["20@rust::main::bb4"];
        assert!(tags.contains(TAINT_ASSIGN));
        assert!(tags.contains(TAINT_C_MALLOC_FAMILY));
    }

    #[test]
    fn malloc_provenance_flows_store_load_phi_without_alloc_certainty() {
        let suffix = "rust::main::bb4";
        let mut mem = AbstractMemory::default();
        let mut taint = TaintStateMap::default();
        taint
            .entry(format!("20@{}", suffix))
            .or_default()
            .extend([
                TAINT_ASSIGN.to_string(),
                TAINT_C_MALLOC_FAMILY.to_string(),
            ]);

        let steps = [
            (
                14,
                stmt(
                    "StoreStmt",
                    "StoreStmt: [Var11 <-- Var20]",
                    Some(11),
                    Some(20),
                    None,
                ),
            ),
            (
                15,
                stmt(
                    "LoadStmt",
                    "LoadStmt: [Var30 <-- Var11]",
                    Some(30),
                    Some(11),
                    None,
                ),
            ),
            (
                2,
                stmt(
                    "PhiStmt",
                    "PhiStmt: [Var6 <-- ([Var30, ICFGNode20],)]",
                    Some(6),
                    None,
                    Some(vec![30]),
                ),
            ),
        ];

        for (node_num, statement) in steps {
            let node_id =
                format!("llvm::c_alloc::node{}::{}", node_num, suffix);
            let stmt_info = statement.stmt_info.clone();
            let node = llvm_node(
                "IntraBlock",
                &stmt_info,
                vec![statement],
            );
            let (next_mem, next_taint) =
                transfer_llvm_node(&node_id, &node, &mem, &taint);
            mem = next_mem;
            taint = next_taint;
        }

        assert!(
            taint["6@rust::main::bb4"]
                .contains(TAINT_C_MALLOC_FAMILY)
        );
        assert_eq!(
            mem.get_cell_value(&"6@rust::main::bb4".to_string()),
            CellValue::TOP
        );
    }

    #[test]
    fn legacy_exporter_phi_schema_propagates_c_malloc_provenance() {
        let raw = r#"{
            "stmt_id": 1,
            "stmt_type": "PhiStmt",
            "stmt_info": "PhiStmt: [Var6 <-- ([Var30, ICFGNode20],)]",
            "edge_id": null,
            "pta_edge": true,
            "res_var_id": 6,
            "operand_vars": [{"op_var_id": 30, "icfg_node": 20}],
            "call_inst": null,
            "is_conditional": null,
            "condition_var_id": null,
            "successors": null
        }"#;
        let phi: SvfStatement = serde_json::from_str(raw).unwrap();
        assert_eq!(phi.result_var_id(), Some(6));
        assert_eq!(llvm_provenance_flow_sources(&phi), vec![30]);

        let mut mem = AbstractMemory::default();
        let mut taint = TaintStateMap::default();
        taint
            .entry("30@rust::main::bb4".to_string())
            .or_default()
            .insert(TAINT_C_MALLOC_FAMILY.to_string());

        let stmt_info = phi.stmt_info.clone();
        let node = llvm_node("FunExitBlock", &stmt_info, vec![phi]);
        (mem, taint) = transfer_llvm_node(
            "llvm::c_alloc::node2::rust::main::bb4",
            &node,
            &mem,
            &taint,
        );

        assert!(
            taint["6@rust::main::bb4"].contains(TAINT_C_MALLOC_FAMILY)
        );
        assert_eq!(
            mem.get_cell_value(&"6@rust::main::bb4".to_string()),
            CellValue::TOP
        );
    }

    #[test]
    fn c_origin_representative_is_invariant_to_phi_operand_order() {
        let c_origin = "30@rust::main::bb4".to_string();
        let other = "40@rust::main::bb4".to_string();
        let origins = HashSet::from([c_origin.clone()]);

        assert_eq!(
            preferred_provenance_source(
                &[c_origin.clone(), other.clone()],
                &origins,
            ),
            Some(c_origin.clone())
        );
        assert_eq!(
            preferred_provenance_source(
                &[other, c_origin.clone()],
                &origins,
            ),
            Some(c_origin)
        );
    }

    #[test]
    fn external_dummyret_materializes_c_allocation_in_rust_return_local() {
        let mut taint = TaintStateMap::default();
        taint
            .entry("6@rust::main::bb4".to_string())
            .or_default()
            .extend([
                TAINT_ASSIGN.to_string(),
                TAINT_C_MALLOC_FAMILY.to_string(),
            ]);

        let dummy = DummyNode {
            dummy_node_name: "dummyRet".to_string(),
            incoming_edge:
                "llvm::c_alloc::node2::rust::main::bb4".to_string(),
            outgoing_edge: "rust::main::bb5".to_string(),
            id: "test".to_string(),
            mir_var: Some("Local(_7) [mutable]".to_string()),
            llvm_var: Some("6@rust::main::bb4".to_string()),
            argument_bindings: Vec::new(),
            is_internal: Some(false),
        };

        let (mem, out_taint) = transfer_dummyret_node(
            &dummy,
            &AbstractMemory::default(),
            &taint,
        );

        assert_eq!(
            mem.get_cell_value(&"Local(_7)".to_string()),
            CellValue::TOP
        );
        assert!(
            out_taint["Local(_7)"]
                .contains(TAINT_C_MALLOC_FAMILY)
        );
        assert!(out_taint["Local(_7)"].contains(TAINT_ASSIGN));
    }

    #[test]
    fn c_malloc_family_is_positive_may_provenance_over_free_flow_group() {
        let mut groups = BTreeMap::new();
        groups.insert(
            "Local(_1)".to_string(),
            "{Local(_1), Local(_2)}".to_string(),
        );

        let origins =
            HashSet::from(["Local(_1)".to_string()]);

        assert!(is_known_c_malloc_family(
            &"Local(_2)".to_string(),
            &groups,
            &origins,
        ));
        assert!(!is_known_c_malloc_family(
            &"Local(_9)".to_string(),
            &groups,
            &origins,
        ));
    }

    #[test]
    fn realloc_is_not_classified_as_phase5_malloc_allocation() {
        assert!(is_c_malloc_family_alloc_call(
            "call ptr @malloc(i64 8)"
        ));
        assert!(is_c_malloc_family_alloc_call(
            "call ptr @calloc(i64 1, i64 8)"
        ));
        assert!(!is_c_malloc_family_alloc_call(
            "call ptr @realloc(ptr %p, i64 16)"
        ));
        assert!(!is_c_malloc_family_alloc_call(
            "call ptr @aligned_alloc(i64 16, i64 32)"
        ));
        assert!(!is_c_malloc_family_alloc_call(
            "call ptr @strdup(ptr %s)"
        ));
    }

    fn edge(a: &str, b: &str) -> IcfgEdge {
        IcfgEdge {
            source: a.to_string(),
            destination: b.to_string(),
            label: None,
            source_label: None,
            destination_label: None,
        }
    }

    #[test]
    fn inlined_c_free_then_reachable_rust_use_is_uaf() {
        let g = GlobalICFGOrdered {
            ordered_nodes: Vec::new(),
            icfg_edges: vec![
                edge("alloc", "c_free"),
                edge("c_free", "dummy_ret"),
                edge("dummy_ret", "rust_use"),
            ],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
        let frees = BTreeSet::from(["c_free".to_string()]);
        let uses = BTreeSet::from(["rust_use".to_string()]);

        assert!(icfg_may_reach(&g, "c_free", "rust_use"));
        assert!(c_malloc_has_use_after_inlined_free(
            &g, &frees, &uses
        ));
    }

    #[test]
    fn use_before_inlined_c_free_is_not_uaf() {
        let g = GlobalICFGOrdered {
            ordered_nodes: Vec::new(),
            icfg_edges: vec![
                edge("alloc", "rust_use"),
                edge("rust_use", "c_free"),
                edge("c_free", "exit"),
            ],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
        let frees = BTreeSet::from(["c_free".to_string()]);
        let uses = BTreeSet::from(["rust_use".to_string()]);

        assert!(!c_malloc_has_use_after_inlined_free(
            &g, &frees, &uses
        ));
    }

    #[test]
    fn mutually_exclusive_free_and_use_are_not_ordered_by_source_position() {
        let g = GlobalICFGOrdered {
            ordered_nodes: Vec::new(),
            icfg_edges: vec![
                edge("entry", "c_free"),
                edge("c_free", "exit"),
                edge("entry", "rust_use"),
                edge("rust_use", "exit"),
            ],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
        let frees = BTreeSet::from(["c_free".to_string()]);
        let uses = BTreeSet::from(["rust_use".to_string()]);

        assert!(!icfg_may_reach(&g, "c_free", "rust_use"));
        assert!(!c_malloc_has_use_after_inlined_free(
            &g, &frees, &uses
        ));
    }

    #[test]
    fn detector_registers_actual_mir_pointer_cast_endpoints_for_direct_c_free() {
        let stmt = assign_stmt(
            "Local(_4)",
            "move _2 as *mut std::ffi::c_void (PtrToPtr)",
        );

        assert_eq!(
            detector_pointer_cast_participants(&stmt),
            Some((
                "Local(_4)".to_string(),
                "Local(_2)".to_string(),
            ))
        );
    }

    #[test]
    fn c_malloc_provenance_does_not_flow_through_pointer_comparison_or_integer_binary() {
        let cmp = stmt(
            "CmpStmt",
            "CmpStmt: [Var40 <-- (Var20 == Var30)]",
            Some(40),
            Some(20),
            Some(vec![20, 30]),
        );
        let bin = stmt(
            "BinaryOPStmt",
            "BinaryOPStmt: [Var41 <-- (Var20 + Var30)]",
            Some(41),
            Some(20),
            Some(vec![20, 30]),
        );

        assert!(llvm_provenance_flow_sources(&cmp).is_empty());
        assert!(llvm_provenance_flow_sources(&bin).is_empty());
    }

    #[test]
    fn external_dummyret_without_positive_c_origin_does_not_invent_allocation() {
        let dummy = DummyNode {
            dummy_node_name: "dummyRet".to_string(),
            incoming_edge:
                "llvm::c_static::node2::rust::main::bb4".to_string(),
            outgoing_edge: "rust::main::bb5".to_string(),
            id: "test-negative".to_string(),
            mir_var: Some("Local(_7) [mutable]".to_string()),
            llvm_var: Some("6@rust::main::bb4".to_string()),
            argument_bindings: Vec::new(),
            is_internal: Some(false),
        };

        let (mem, taint) = transfer_dummyret_node(
            &dummy,
            &AbstractMemory::default(),
            &TaintStateMap::default(),
        );

        assert_eq!(
            mem.get_cell_value(&"Local(_7)".to_string()),
            CellValue::BOTTOM
        );
        assert!(!taint
            .get("Local(_7)")
            .map(|t| t.contains(TAINT_C_MALLOC_FAMILY))
            .unwrap_or(false));
    }

    #[test]
    fn bare_free_requires_foreign_declaration_but_libc_path_is_recognized() {
        let mut ffi = HashSet::new();

        assert!(!is_c_free_function("free", &ffi));
        assert!(!is_c_free_function("my_crate::free", &ffi));

        ffi.insert("free".to_string());
        assert!(is_c_free_function("free", &ffi));
        assert!(is_c_free_call_text(
            "free(copy _1) -> [return: bb1, unwind continue]",
            &ffi
        ));

        let empty = HashSet::new();
        assert!(is_c_free_function("libc::unix::free", &empty));
        assert!(!is_c_free_function("std::alloc::dealloc", &empty));
    }

    #[test]
    fn direct_c_free_counts_each_explicit_call() {
        let mut info = VarInfo::new();
        info.c_free_mir = 1;
        assert_eq!(info.effective_free(), 1);

        info.c_free_mir = 2;
        assert_eq!(info.effective_free(), 2);
    }

    #[test]
    fn direct_c_free_is_distinct_from_legacy_llvm_drop_deduplication() {
        let mut info = VarInfo::new();
        info.llvm_free = 1;
        info.drop_free = 1;
        info.c_free_mir = 1;

        // LLVM+Drop is the historical single deallocation event; the explicit
        // C free is a second event.
        assert_eq!(info.effective_free(), 2);
    }

    #[test]
    fn c_malloc_origin_flags_unproven_owning_contracts_but_not_borrows() {
        assert!(
            c_malloc_rust_allocator_contract_warning(
                "std::ffi::CString::from_raw"
            )
            .is_some()
        );
        assert!(
            c_malloc_rust_allocator_contract_warning(
                "std::boxed::Box::<i32>::from_raw"
            )
            .is_some()
        );
        assert!(
            c_malloc_rust_allocator_contract_warning(
                "std::vec::Vec::<i32>::from_raw_parts"
            )
            .is_some()
        );
        assert!(
            c_malloc_rust_allocator_contract_warning(
                "std::string::String::from_raw_parts"
            )
            .is_some()
        );
        assert!(
            c_malloc_rust_allocator_contract_warning("std::alloc::dealloc")
                .is_some()
        );
        assert!(
            c_malloc_rust_allocator_contract_warning(
                "std::ffi::CStr::from_ptr::<'_>"
            )
            .is_none()
        );
    }
}


#[cfg(test)]
mod phase4_std_memory_tests {
    use super::{
        apply_mir_statement, apply_mir_terminator, eval_rvalue, leak_ghost_name, transfer_call,
        AbstractMemory, CellValue, TaintStateMap, first_call_local_from_details,
        direct_stack_borrow_source,
        direct_deref_value_source,
        resolve_stack_ref_values,
        closure_aggregate_capture_operands,
        closure_field_copy_for_deref_index,
        is_copy_for_deref_rvalue,
        is_closure_aggregate_rvalue,
        mir_function_scope_from_node_id,
        resolve_scoped_stack_ref_leaves,
        is_direct_deref_use_rvalue,
        is_raw_pointer_cast_method_call,
        TAINT_ASSIGN,
        TAINT_C_MALLOC_FAMILY
    };
    use std::collections::{BTreeMap, BTreeSet};
    use crate::structs::{MirCallArgument, MirStatement, MirTerminator, SourceInfoData, RustAllocationDispositionEvidence, RustAllocationDispositionEvidenceKind};

    fn n(s: &str) -> String {
        s.to_string()
    }

    fn assign_stmt(place: &str, rvalue: &str) -> MirStatement {
        MirStatement {
            source_info: SourceInfoData {
                span: "<phase4-test>".to_string(),
                scope: "<phase4-test>".to_string(),
            },
            kind: "Assign".to_string(),
            details: format!("Assign(({}, {}))", place, rvalue),
            place: Some(place.to_string()),
            is_mutable: Some(true),
            rvalue: Some(rvalue.to_string()),
        }
    }

    #[test]
    fn generic_box_new_creates_fresh_allocation() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_9)"), CellValue::ALLOC);

        let details =
            "std::boxed::Box::<crate::Nested<Vec<i32>>>::new(move _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::ALLOC);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::ALLOC);
        assert_ne!(
            mem.get_allocation(&n("Local(_2)")),
            mem.get_allocation(&n("Local(_9)"))
        );
    }

    #[test]
    fn generic_box_into_raw_preserves_existing_alias_component() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);
        mem.propagate_cell_value(&n("Local(_1)"), &n("Local(_9)"));

        let details =
            "std::boxed::Box::<crate::Nested<Vec<i32>>>::into_raw(move _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::MV);
        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::BOTTOM);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::MV);
        assert_eq!(
            mem.get_allocation(&n("Local(_2)")),
            mem.get_allocation(&n("Local(_9)"))
        );
    }

    #[test]
    fn generic_box_from_raw_keeps_raw_alias_for_double_free_detection() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let details =
            "std::boxed::Box::<crate::Nested<Vec<i32>>>::from_raw(copy _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::MV);
        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn pointer_comparison_is_scalar_even_for_raw_allocation_values() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);
        mem.set_cell_value(&n("Local(_2)"), CellValue::MV);

        assert_eq!(
            eval_rvalue("Eq(copy _1, copy _2)", &mem),
            CellValue::BOXTIMES
        );
    }

    #[test]
    fn generic_pointer_cast_preserves_tracked_allocation_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);
        let mut taint = TaintStateMap::default();
        taint
            .entry(n("Local(_1)"))
            .or_default()
            .insert("assign".to_string());

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt(
                "Local(_2)",
                "copy _1 as *mut crate::Node<i32> (PtrToPtr)",
            ),
        );

        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::MV);
    }

    #[test]
    fn moved_pointer_cast_preserves_tracked_allocation_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_2)"), CellValue::TOP);
        let mut taint = TaintStateMap::default();
        taint
            .entry(n("Local(_2)"))
            .or_default()
            .extend([TAINT_ASSIGN.to_string(), TAINT_C_MALLOC_FAMILY.to_string()]);

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt(
                "Local(_1)",
                "move _2 as *mut u8 (PtrToPtr)",
            ),
        );

        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
        assert!(taint
            .get("Local(_1)")
            .is_some_and(|tags| tags.contains(TAINT_C_MALLOC_FAMILY)));
    }

    #[test]
    fn raw_pointer_cast_method_call_preserves_alias_and_c_origin() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::TOP);
        let mut taint = TaintStateMap::default();
        taint
            .entry(n("Local(_1)"))
            .or_default()
            .extend([TAINT_ASSIGN.to_string(), TAINT_C_MALLOC_FAMILY.to_string()]);

        let term = MirTerminator::Call {
            details: "_11 = std::ptr::mut_ptr::<impl *mut i32>::cast::<std::ffi::c_void>(copy _1)".to_string(),
            source_info: "<phase5-real-shape>".to_string(),
            function_called: "std::ptr::mut_ptr::<impl *mut i32>::cast::<std::ffi::c_void>".to_string(),
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
            arguments: vec![MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            return_place: "_11".to_string(),
            return_target: Some("bb7".to_string()),
            unwind_target: "unreachable".to_string(),
        };

        let mem = apply_mir_terminator(&mem, &mut taint, &term);

        assert!(is_raw_pointer_cast_method_call(
            "std::ptr::mut_ptr::<impl *mut i32>::cast::<std::ffi::c_void>"
        ));
        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_11)"))
        );
        assert!(taint
            .get("Local(_11)")
            .is_some_and(|tags| tags.contains(TAINT_C_MALLOC_FAMILY)));
    }

    #[test]
    fn integer_to_pointer_cast_is_top_not_heap_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::BOXTIMES);
        let mut taint = TaintStateMap::default();

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "copy _1 as *mut i32 (IntToPtr)"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::TOP);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn direct_reference_to_owner_is_stack_place_not_heap_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);

        let mut taint = TaintStateMap::default();
        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "&_1"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::ALLOC);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::TOP);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn direct_reference_to_raw_handle_is_stack_place_not_heap_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let mut taint = TaintStateMap::default();
        taint
            .entry(n("Local(_1)"))
            .or_default()
            .insert("assign".to_string());

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "&_1"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::MV);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::TOP);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn stack_borrow_parser_distinguishes_direct_place_from_deref_borrow() {
        assert_eq!(
            direct_stack_borrow_source("&_7"),
            Some(n("Local(_7)"))
        );
        assert_eq!(
            direct_stack_borrow_source("&mut _7"),
            Some(n("Local(_7)"))
        );
        assert_eq!(
            direct_stack_borrow_source("&raw const _7"),
            Some(n("Local(_7)"))
        );
        assert_eq!(direct_stack_borrow_source("&(*_7)"), None);
    }

    #[test]
    fn direct_deref_parser_recovers_reference_local() {
        assert_eq!(
            direct_deref_value_source("copy (*_4)"),
            Some(n("Local(_4)"))
        );
        assert_eq!(
            direct_deref_value_source("move (*_9)"),
            Some(n("Local(_9)"))
        );
        assert_eq!(direct_deref_value_source("copy ((*_4).0: i32)"), None);
    }

    #[test]
    fn stack_reference_resolution_is_a_may_relation() {
        let mut refs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        refs.entry(n("Local(_4)"))
            .or_default()
            .insert(n("Local(_2)"));
        refs.entry(n("Local(_4)"))
            .or_default()
            .insert(n("Local(_8)"));

        let targets = resolve_stack_ref_values(&n("Local(_4)"), &refs);
        assert_eq!(
            targets,
            BTreeSet::from([n("Local(_2)"), n("Local(_8)")])
        );
    }

    #[test]
    fn closure_aggregate_parser_preserves_capture_field_order() {
        let rv =
            "{closure@/tmp/main.rs:33:23: 33:25} { ptr: move _4, len: copy _7 }";

        assert_eq!(
            closure_aggregate_capture_operands(rv),
            Some(vec![n("Local(_4)"), n("Local(_7)")])
        );
    }

    #[test]
    fn closure_copy_for_deref_parser_recognizes_observed_mir_spellings() {
        assert_eq!(
            closure_field_copy_for_deref_index(
                "deref_copy ((*_1).0: &*mut i32)"
            ),
            Some(0)
        );
        assert_eq!(
            closure_field_copy_for_deref_index(
                "deref_copy (_1.3: &LockFreeStack<i32>)"
            ),
            Some(3)
        );
        assert_eq!(
            closure_field_copy_for_deref_index("deref_copy (*_18)"),
            None
        );
    }

    #[test]
    fn closure_and_copy_for_deref_are_positive_top_not_bottom() {
        let mem = AbstractMemory::default();

        assert!(is_closure_aggregate_rvalue(
            "{closure@/tmp/main.rs:1:1: 1:3} { ptr: move _4 }"
        ));
        assert_eq!(
            eval_rvalue(
                "{closure@/tmp/main.rs:1:1: 1:3} { ptr: move _4 }",
                &mem
            ),
            CellValue::TOP
        );

        assert!(is_copy_for_deref_rvalue(
            "deref_copy ((*_1).0: &*mut i32)"
        ));
        assert_eq!(
            eval_rvalue("deref_copy ((*_1).0: &*mut i32)", &mem),
            CellValue::TOP
        );
    }

    #[test]
    fn direct_deref_use_is_positive_top_not_bottom() {
        let mem = AbstractMemory::default();

        assert!(is_direct_deref_use_rvalue("copy (*_8)"));
        assert!(is_direct_deref_use_rvalue("move (*_9)"));
        assert!(!is_direct_deref_use_rvalue("copy _8"));

        assert_eq!(
            eval_rvalue("copy (*_8)", &mem),
            CellValue::TOP
        );
    }

    #[test]
    fn scoped_stack_reference_resolution_reaches_outer_value_leaf() {
        let mut refs: BTreeMap<(String, String), BTreeSet<String>> =
            BTreeMap::new();
        refs.entry(("rust::main".to_string(), n("Local(_4)")))
            .or_default()
            .insert(n("Local(_2)"));

        assert_eq!(
            resolve_scoped_stack_ref_leaves(
                "rust::main",
                &n("Local(_4)"),
                &refs
            ),
            BTreeSet::from([n("Local(_2)")])
        );

        // A normal by-value local is not misclassified as a reference.
        assert!(
            resolve_scoped_stack_ref_leaves(
                "rust::main",
                &n("Local(_7)"),
                &refs
            )
            .is_empty()
        );
    }

    #[test]
    fn mir_scope_parser_is_function_and_closure_aware() {
        assert_eq!(
            mir_function_scope_from_node_id("rust::main::bb2"),
            Some("rust::main".to_string())
        );
        assert_eq!(
            mir_function_scope_from_node_id(
                "rust::main::{closure#0}::bb3"
            ),
            Some("rust::main::{closure#0}".to_string())
        );
    }

    #[test]
    fn raw_address_does_not_alias_managed_heap_allocation() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);
        let mut taint = TaintStateMap::default();

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "&raw const _1"),
        );

        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::TOP);
        assert_ne!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn pointer_offset_preserves_allocation_alias() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);
        mem.set_cell_value(&n("Local(_3)"), CellValue::BOXTIMES);
        let mut taint = TaintStateMap::default();

        let mem = apply_mir_statement(
            &mem,
            &mut taint,
            &assign_stmt("Local(_2)", "Offset(copy _1, copy _3)"),
        );

        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn mem_forget_returns_unit_and_keeps_leak_witness() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);

        let details =
            "std::mem::forget::<std::boxed::Box<i32>>(move _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_0");

        assert_eq!(ret, CellValue::BOXTIMES);
        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::BOTTOM);

        let ghost = leak_ghost_name(&n("Local(_1)"));
        assert_eq!(mem.get_cell_value(&ghost), CellValue::MV);
    }

    #[test]
    fn box_leak_consumes_owner_and_retains_leaked_allocation() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);

        let details =
            "std::boxed::Box::<i32>::leak::<'_>(move _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::MV);
        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::BOTTOM);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::MV);
    }

    #[test]
    fn call_operand_parser_strips_comma_from_first_copied_argument() {
        let details =
            "std::vec::Vec::<i32>::from_raw_parts(copy _1, copy _2, copy _3) -> [return: bb1, unwind continue]";

        assert_eq!(
            first_call_local_from_details(details),
            Some(n("Local(_1)"))
        );
    }

    #[test]
    fn call_operand_parser_selects_first_operand_independent_of_move_copy_kind() {
        let copy_then_move =
            "foo(copy _3, move _4) -> [return: bb1, unwind continue]";
        let move_then_copy =
            "foo(move _5, copy _6) -> [return: bb1, unwind continue]";

        assert_eq!(
            first_call_local_from_details(copy_then_move),
            Some(n("Local(_3)"))
        );
        assert_eq!(
            first_call_local_from_details(move_then_copy),
            Some(n("Local(_5)"))
        );
    }

    #[test]
    fn vec_from_raw_parts_reuses_raw_allocation_component() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let details =
            "std::vec::Vec::<i32>::from_raw_parts(copy _1, copy _2, copy _3) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_4");

        assert_eq!(ret, CellValue::MV);
        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_4)"))
        );
    }

    #[test]
    fn vec_as_mut_ptr_is_borrowed_raw_view_not_ownership_transfer() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::ALLOC);

        let details =
            "std::vec::Vec::<i32>::as_mut_ptr(move _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::ALLOC);
        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::ALLOC);
        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
    }

    #[test]
    fn raw_alloc_is_top_because_alloc_may_return_null() {
        let mem = AbstractMemory::default();
        let details =
            "std::alloc::alloc(move _1) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::TOP);
        assert_eq!(mem.get_cell_value(&n("Local(_2)")), CellValue::TOP);
    }

    #[test]
    fn raw_dealloc_frees_tracked_raw_allocation_and_returns_unit() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let details =
            "std::alloc::dealloc(copy _1, copy _2) -> [return: bb1, unwind continue]";
        let (ret, mem) = transfer_call(&mem, details, "_0");

        assert_eq!(ret, CellValue::BOXTIMES);
        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::FREED);
    }

    #[test]
    fn cstr_from_ptr_is_borrow_not_ownership_transfer() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);
        let mut taint = TaintStateMap::default();
        taint
            .entry(n("Local(_1)"))
            .or_default()
            .insert("assign".to_string());

        let term = MirTerminator::Call {
            details: "_2 = std::ffi::CStr::from_ptr::<'_>(copy _1)".to_string(),
            source_info: "<phase4-test>".to_string(),
            function_called: "std::ffi::CStr::from_ptr::<'_>".to_string(),
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
            arguments: vec![MirCallArgument {
                arg: "Local(_1) [mutable]".to_string(),
                is_mutable: Some(true),
            }],
            return_place: "_2".to_string(),
            return_target: Some("bb1".to_string()),
            unwind_target: "continue".to_string(),
        };

        let mem = apply_mir_terminator(&mem, &mut taint, &term);

        assert_eq!(mem.get_cell_value(&n("Local(_1)")), CellValue::MV);
        assert_eq!(
            mem.get_allocation(&n("Local(_1)")),
            mem.get_allocation(&n("Local(_2)"))
        );
        assert!(taint
            .get("Local(_2)")
            .is_some_and(|tags| tags.contains("assign")));
    }

    #[test]
    fn raw_realloc_widens_source_alias_component_and_nullable_result() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);
        mem.propagate_cell_value(&n("Local(_1)"), &n("Local(_5)"));

        let details =
            "std::alloc::realloc(copy _1, copy _2, copy _3) -> [return: bb1, unwind continue]";
        let (ret, mem2) = transfer_call(&mem, details, "_4");

        assert_eq!(ret, CellValue::TOP);
        assert_eq!(mem2.get_cell_value(&n("Local(_1)")), CellValue::TOP);
        assert_eq!(mem2.get_cell_value(&n("Local(_5)")), CellValue::TOP);
        assert_eq!(mem2.get_cell_value(&n("Local(_4)")), CellValue::TOP);
        assert_ne!(
            mem2.get_allocation(&n("Local(_1)")),
            mem2.get_allocation(&n("Local(_4)")),
            "realloc success may move, and failure returns null; the return must not be forced into the old alias component"
        );
    }

    #[test]
    fn ptr_write_does_not_claim_backing_allocation_was_freed() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let details =
            "std::ptr::write::<i32>(copy _1, move _2) -> [return: bb1, unwind continue]";
        let (ret, mem2) = transfer_call(&mem, details, "_0");

        assert_eq!(ret, CellValue::BOXTIMES);
        assert_eq!(mem2.get_cell_value(&n("Local(_1)")), CellValue::MV);
    }

    #[test]
    fn ptr_drop_in_place_is_not_equated_with_backing_deallocation() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let details =
            "std::ptr::drop_in_place::<i32>(copy _1) -> [return: bb1, unwind continue]";
        let (ret, mem2) = transfer_call(&mem, details, "_0");

        assert_eq!(ret, CellValue::BOXTIMES);
        assert_eq!(mem2.get_cell_value(&n("Local(_1)")), CellValue::MV);
    }

    #[test]
    fn ptr_read_does_not_free_source_allocation() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);

        let details =
            "std::ptr::read::<i32>(copy _1) -> [return: bb1, unwind continue]";
        let (ret, mem2) = transfer_call(&mem, details, "_2");

        assert_eq!(ret, CellValue::TOP);
        assert_eq!(mem2.get_cell_value(&n("Local(_1)")), CellValue::MV);
    }
    #[test]
    fn v6s_certified_raw_pointer_mem_drop_preserves_pointee_allocation() {
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&n("Local(_1)"), CellValue::MV);
        let mut taint = TaintStateMap::default();
        taint.entry(n("Local(_1)")).or_default().insert(TAINT_ASSIGN.to_string());

        // Intentionally avoid a pretty-printed *mut type in function_called.
        // v6S must rely on producer-certified rustc type evidence, not text.
        let term = MirTerminator::Call {
            details: "_0 = core::mem::drop::<opaque>(copy _1)".to_string(),
            source_info: "<v6s-raw-drop-test>".to_string(),
            function_called: "core::mem::drop::<opaque>".to_string(),
            callee_def_path: Some("core::mem::drop".to_string()),
            deallocator_evidence: None,
            allocation_disposition_evidence: Some(RustAllocationDispositionEvidence {
                kind: RustAllocationDispositionEvidenceKind::MemDropRawPointer,
                callee_def_path: "core::mem::drop".to_string(),
                owner_def_path: None,
            }),
            higher_order_evidence: None,
            callee_is_local: false,
            callback_def_paths: Vec::new(),
            resolved_instance_callees: Vec::new(),
            instance_dispatch_observed: false,
            instance_dispatch_external: false,
            instance_dispatch_unresolved: false,
            arguments: vec![MirCallArgument {
                arg: "Local(_1)".to_string(),
                is_mutable: Some(false),
            }],
            return_place: "_0".to_string(),
            return_target: Some("bb1".to_string()),
            unwind_target: "continue".to_string(),
        };

        let after = apply_mir_terminator(&mem, &mut taint, &term);
        assert_eq!(after.get_cell_value(&n("Local(_1)")), CellValue::MV);
        assert_eq!(after.get_cell_value(&n("Local(_0)")), CellValue::BOXTIMES);
        assert!(!taint.get("Local(_1)").is_some_and(|tags| tags.contains("free")));
    }

}

#[cfg(test)]
mod v6p_mir_extension_tests {
    use super::*;
    use crate::structs::SourceInfoData;

    fn assign(place: &str, rvalue: &str) -> MirStatement {
        MirStatement {
            source_info: SourceInfoData { span: "test".into(), scope: "test".into() },
            kind: "Assign".into(),
            details: format!("{place} = {rvalue}"),
            place: Some(place.into()),
            is_mutable: Some(true),
            rvalue: Some(rvalue.into()),
        }
    }

    #[test]
    fn checked_arithmetic_extension_is_non_owning_scalar() {
        let mem = AbstractMemory::default();
        let mut taint = TaintStateMap::new();
        let out = transfer_unmodeled_assign_v2(
            &mem,
            &mut taint,
            &assign("Local(_1)", "AddWithOverflow(copy _2, copy _3)"),
        ).unwrap();
        assert_eq!(out.get_cell_value(&full_local_name("Local(_1)")), CellValue::BOXTIMES);
    }

    #[test]
    fn pointer_metadata_extension_widens_to_top_not_heap_fact() {
        let mem = AbstractMemory::default();
        let mut taint = TaintStateMap::new();
        let out = transfer_unmodeled_assign_v2(
            &mem,
            &mut taint,
            &assign("Local(_1)", "PtrMetadata(copy _2)"),
        ).unwrap();
        assert_eq!(out.get_cell_value(&full_local_name("Local(_1)")), CellValue::TOP);
    }

    fn stmt(kind: &str, place: Option<&str>, details: &str) -> MirStatement {
        MirStatement {
            source_info: SourceInfoData { span: "test".into(), scope: "test".into() },
            kind: kind.into(),
            details: details.into(),
            place: place.map(|p| p.to_string()),
            is_mutable: Some(true),
            rvalue: None,
        }
    }

    #[test]
    fn storage_live_is_fresh_uninitialized_not_bottom() {
        let mut mem = AbstractMemory::default();
        let key = full_local_name("Local(_1)");
        mem.assign_local_value(&key, CellValue::ALLOC);
        let mut taint = TaintStateMap::new();
        let out = transfer_storage_live_v2(
            &mem,
            &mut taint,
            &stmt("StorageLive", Some("Local(_1)"), "StorageLive(_1)"),
        ).unwrap();
        assert_eq!(out.get_cell_value(&key), CellValue::BOXTIMES);
        assert_ne!(out.get_cell_value(&key), CellValue::BOTTOM);
    }

    #[test]
    fn storage_dead_removes_stack_slot_without_freeing_heap_alias() {
        let mut mem = AbstractMemory::default();
        let local = full_local_name("Local(_1)");
        let alias = full_local_name("Local(_2)");
        mem.set_cell_value(&local, CellValue::ALLOC);
        mem.propagate_cell_value(&local, &alias);
        let mut taint = TaintStateMap::new();
        let out = transfer_storage_dead_v2(
            &mem,
            &mut taint,
            &stmt("StorageDead", Some("Local(_1)"), "StorageDead(_1)"),
        ).unwrap();
        assert_eq!(out.get_cell_value(&local), CellValue::BOTTOM);
        assert_eq!(out.get_cell_value(&alias), CellValue::ALLOC);
        assert_ne!(out.get_cell_value(&alias), CellValue::FREED);
    }

    #[test]
    fn exact_local_deinit_becomes_uninitialized_not_absent_or_freed() {
        let mut mem = AbstractMemory::default();
        let key = full_local_name("Local(_1)");
        mem.assign_local_value(&key, CellValue::ALLOC);
        let mut taint = TaintStateMap::new();
        let statement = stmt("Deinit", Some("Local(_1)"), "Deinit(Local(_1))");
        let out = transfer_deinit_v2(&mem, &mut taint, &statement).unwrap();
        assert_eq!(out.get_cell_value(&key), CellValue::BOXTIMES);
        assert_ne!(out.get_cell_value(&key), CellValue::BOTTOM);
        assert_ne!(out.get_cell_value(&key), CellValue::FREED);
    }

    #[test]
    fn projected_deinit_widens_root_allocation_component() {
        let mut mem = AbstractMemory::default();
        let key = full_local_name("Local(_1)");
        mem.assign_local_value(&key, CellValue::ALLOC);
        let mut taint = TaintStateMap::new();
        let statement = stmt(
            "Deinit",
            Some("Local(_1) [mutable] -> Field(0, Type: i32)"),
            "Deinit((_1.0: i32))",
        );
        let out = transfer_deinit_v2(&mem, &mut taint, &statement).unwrap();
        assert_eq!(out.get_cell_value(&key), CellValue::TOP);
    }

    #[test]
    fn set_discriminant_and_retag_are_supported_by_sound_widening() {
        let mut mem = AbstractMemory::default();
        let key = full_local_name("Local(_1)");
        mem.assign_local_value(&key, CellValue::ALLOC);
        let sd = stmt("SetDiscriminant", Some("Local(_1)"), "SetDiscriminant(_1, 1)");
        let rt = stmt("Retag", Some("Local(_1)"), "Retag(_1)");
        assert_eq!(transfer_opaque_place_write_v2(&mem, &sd).get_cell_value(&key), CellValue::TOP);
        assert_eq!(transfer_opaque_place_write_v2(&mem, &rt).get_cell_value(&key), CellValue::TOP);
    }

    #[test]
    fn assume_retains_state_as_may_overapproximation() {
        let mut mem = AbstractMemory::default();
        let key = full_local_name("Local(_1)");
        mem.assign_local_value(&key, CellValue::ALLOC);
        let statement = stmt("Intrinsic", None, "Intrinsic::Assume Assume(copy _2)");
        let out = transfer_intrinsic_v2(&mem, &statement);
        assert_eq!(out.get_cell_value(&key), CellValue::ALLOC);
    }

    #[test]
    fn copy_nonoverlapping_globally_widens_without_points_to_proof() {
        let mut mem = AbstractMemory::default();
        let one = full_local_name("Local(_1)");
        let two = full_local_name("Local(_2)");
        mem.assign_local_value(&one, CellValue::ALLOC);
        mem.assign_local_value(&two, CellValue::MV);
        let statement = stmt(
            "Intrinsic",
            Some("Local(_2)"),
            "Intrinsic::CopyNonOverlapping CopyNonOverlapping { .. }",
        );
        let out = transfer_intrinsic_v2(&mem, &statement);
        assert_eq!(out.get_cell_value(&one), CellValue::TOP);
        assert_eq!(out.get_cell_value(&two), CellValue::TOP);
    }
}
