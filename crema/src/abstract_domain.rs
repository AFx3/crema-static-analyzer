use std::cmp::Ordering;                     // for comparing abstract states
use std::collections::{BTreeSet, HashMap};  // store sets of variable names (for aliasing) and map allocations to cell states
use std::fmt::{self};
use log::debug;
use crate::utils::load_ffi_functions; // load_ffi_functions in utils.rs
use std::collections::HashSet;
use crate::structs::GlobalICFGNode;
use crate::structs::{MirStatement, MirTerminator, MirBasicBlock};
use crate::structs::LlvmJsonNode;
use crate::structs::SvfStatement;
use std::collections::VecDeque;
use once_cell::sync::Lazy;
use regex::Regex;
use crate::structs::{GlobalICFGOrdered,DummyNode};
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
    };

    taint_state.get(&key).cloned().unwrap_or_default()
}


// ----------------------------------------------------------------------
// TRANSFER FUNCTION DISPATCH
// ----------------------------------------------------------------------
pub fn transfer_function(node: &GlobalICFGNode, in_mem: &AbstractMemory, in_taint: &TaintStateMap) -> (AbstractMemory, TaintStateMap) {
    match node {
        GlobalICFGNode::Mir(bb) => process_mir_basic_block(bb, in_mem, in_taint),
        GlobalICFGNode::DummyCall(dummy_call) => transfer_dummycall_node(dummy_call, in_mem, in_taint),
        GlobalICFGNode::DummyRet(dummy_ret) => transfer_dummyret_node(dummy_ret, in_mem, in_taint),
        GlobalICFGNode::Llvm(llvm_node) => transfer_llvm_node(llvm_node, in_mem, in_taint),
        
       // _ => (in_mem.clone(), in_taint.clone()),
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

pub fn transfer_dummyret_node(dummy: &DummyNode, in_mem: &AbstractMemory, in_taint: &TaintStateMap) -> (AbstractMemory, TaintStateMap) {
    let mut mem_ret   = in_mem.clone();
    let mut taint_ret = in_taint.clone();

    eprintln!("--- ENTER DummyRet {:?}", dummy);

    if dummy.is_internal.unwrap_or(false) {
        if let (Some(mir_var), Some(llvm_var)) = (&dummy.mir_var, &dummy.llvm_var) {
            // normalize var names
            let full_mir  = normalize(mir_var);
            let full_llvm = normalize(llvm_var);

            eprintln!("DUMMYRET normalize: '{}'->'{}', '{}'->'{}'", mir_var, full_mir, llvm_var, full_llvm);

            // leggo i due valori
            let old = mem_ret.get_cell_value(&full_mir);
            let ret = mem_ret.get_cell_value(&full_llvm);
            eprintln!("BEFORE JOIN: {}={:?}, {}={:?}", full_mir, old, full_llvm, ret);

            // join e and write
            let j = old.join(ret);
            eprintln!("JOINED {}={:?}", full_mir, j);
            mem_ret.set_cell_value(&full_mir, j);
            eprintln!("AFTER SET: mem_ret[{}]={:?}", full_mir, mem_ret.get_cell_value(&full_mir));

            // taint similary manages
            let mut t0 = taint_ret.remove(&full_mir).unwrap_or_default();
            let t1 = taint_ret.remove(&full_llvm).unwrap_or_default();
            t0.extend(t1.into_iter());
            taint_ret.insert(full_mir.clone(), t0.clone());
            eprintln!("TAINT {}={:?}", full_mir, t0);
        }
    }

    // non serve nemmeno fare union, per ora (credo!!!!) da testare
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
// it will evaluate an rvalue using the current abstract memory
pub fn eval_rvalue(e: &str, sigma: &AbstractMemory) -> CellValue {
    let trimmed = e.trim();
    if trimmed.starts_with("& imm") {
        CellValue::IMMB
    } else if trimmed.starts_with("& mut") {
        CellValue::MB
    } else {
        sigma.get_cell_value(&e.to_string())
    }
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
// MIR STATEMENT/TERMINATOR DISPATCH
// ----------------------------------------------------------------------
// process an entire MIR basic block (which may consist of zero or more statements followed by a terminator)
// returns a tuple of the updated AbstractMemory and TaintStateMap
pub fn process_mir_basic_block(block: &MirBasicBlock, init_mem: &AbstractMemory, init_taint: &TaintStateMap) -> (AbstractMemory, TaintStateMap) {
    let mut current_mem = init_mem.clone();
    let mut current_taint = init_taint.clone();
    for stmt in &block.statements {
        current_mem = apply_mir_statement(&current_mem, &mut current_taint, stmt);
    }
    if let Some(term) = &block.terminator {
        current_mem = apply_mir_terminator(&current_mem, &mut current_taint, term);
    }
    (current_mem, current_taint)

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
static MEM_FORGET_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::mem::forget::<std::boxed::Box<[^>]+>>").unwrap()
});

static MEM_FORGET_VEC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::mem::forget::<std::vec::Vec<[^>]+>>").unwrap()
});

static MEM_FORGET_BOX_VEC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::mem::forget::<Box<Vec<[^>]+>>").unwrap()
});


////////////////////////////////////////////////////////////////////////////////
//into raw regex
static INTO_RAW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<[^>]+>::into_raw").unwrap()
});

static BOX_VEC_INTO_RAW_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"std::boxed::Box::<std::vec::Vec<[^>]+>>::into_raw").unwrap()
});

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

pub fn transfer_call(mem: &AbstractMemory, func_call_details: &str, return_place: &str) -> (CellValue, AbstractMemory) {
    let _ffi_functions = match load_ffi_functions("./ffi_functions.json") {
        Ok(set) => set,
        Err(e) => {
            eprintln!("Failed to load FFI functions: {:?}", e);
            std::collections::HashSet::new()
        }
    };

    // ALLOC 
    let mut new_mem = mem.clone();
    let ret_val = if

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
    || func_call_details.contains("std::slice::<impl [i32]>::into_vec::<std::alloc::Global>")
    || func_call_details.contains("std::slice::<impl [&str]>::into_vec::<std::alloc::Global>")
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
    ||  INTO_RAW_REGEX.is_match(func_call_details) 
    || BOX_VEC_INTO_RAW_REGEX.is_match(func_call_details)
    || VEC_INTO_RAW_REGEX.is_match(func_call_details)
    || BOX_INTO_RAW_NODE_GENERIC_REGEX.is_match(func_call_details)

    //|| func_call_details.contains("std::boxed::Box::<Node<u32>>::into_raw")

    //vec
    || func_call_details.contains("std::boxed::Box::<std::vec::Vec<char>>::into_raw")
    // MV option<F>
    || func_call_details.contains("std::boxed::Box::<std::option::Option<F>>::into_raw")
    // MV MSG SENDER
    || func_call_details.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::into_raw")

    // MEM FORGET
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<u8>>")   || func_call_details.contains("std::mem::forget::<std::boxed::Box<i8>>")
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<i16>>") || func_call_details.contains("std::mem::forget::<std::boxed::Box<u16>>")
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<i32>>")  || func_call_details.contains("std::mem::forget::<std::boxed::Box<u32>>") 
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<i64>>") || func_call_details.contains("std::mem::forget::<std::boxed::Box<u64>>")
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<i128>>") || func_call_details.contains("std::mem::forget::<std::boxed::Box<u128>>")
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<f32>>") || func_call_details.contains("std::mem::forget::<std::boxed::Box<f64>>") 
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<bool>>") || func_call_details.contains("std::mem::forget::<std::boxed::Box<char>>")
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<usize>>") || func_call_details.contains("std::mem::forget::<std::boxed::Box<isize>>")

    || func_call_details.contains("std::mem::forget::<std::boxed::Box<std::string::String>>")
    || func_call_details.contains("std::mem::forget::<std::ffi::CString>")
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<std::ffi::CString>>")
   
    || func_call_details.contains("std::mem::forget::<std::boxed::Box<&str>>") 

    // Mem forget regex
    ||  MEM_FORGET_REGEX.is_match(func_call_details) 
    || MEM_FORGET_VEC_REGEX.is_match(func_call_details)
    || MEM_FORGET_BOX_VEC_REGEX.is_match(func_call_details)

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
            CellValue::BOTTOM
        } else {
            // For an owning value, free ONLY the tracked allocation that
            // contains the dropped argument. Never mark the entire abstract
            // memory as FREED.
            if let Some(full_dropped) = explicit_drop_arg_from_details(func_call_details) {
                if new_mem.get_allocation(&full_dropped).is_some() {
                    new_mem = update_state(new_mem, &full_dropped, CellValue::FREED);
                }
            }

            // std::mem::drop returns (), so there is no heap value to assign
            // to its MIR return place.
            CellValue::BOTTOM
        }
    } else {
        // default: do nothing
        let full_ret = full_local_name(return_place);
        new_mem.get_cell_value(&full_ret)
        //CellValue::BOTTOM
    };
    (ret_val, new_mem)
}



pub fn apply_mir_statement(mem: &AbstractMemory, taint: &mut TaintStateMap, stmt: &MirStatement) -> AbstractMemory {
    let mut new_mem = mem.clone();
    match stmt.kind.as_str() {
        "Nop" => { /* no change */ }
        "Assign" => {
            if let Some(rvalue) = &stmt.rvalue {
                // RAW REFERENCE CAPTURE: if the rvalue is a reference to a variable -> propagate the cell value
                // e.g. Assign((_4, &_2)) in closure lowering
                // for example within a closure, the expression Assign((_4, &_2)) merges Local(_2) and Local(_4) into one allocation key
                if rvalue.starts_with('&') {
                        if let Some(dest) = &stmt.place {
                            // extract the var after the &
                            let src_var = rvalue.trim_start_matches('&').trim();
                            let src_key = full_local_name(src_var);
                            let dest_key = full_local_name(dest);
                            // unify the allocation sets
                            new_mem.propagate_cell_value(&src_key, &dest_key);
                            new_mem.propagate_cell_value(&dest_key, &src_key);
    
                            // unify taint as well
                            let t_src = taint.get(&src_key).cloned().unwrap_or_default();
                            let t_dest = taint.get(&dest_key).cloned().unwrap_or_default();
                            let joined = t_src.union(&t_dest).cloned().collect::<HashSet<_>>();
                            taint.insert(src_key.clone(), joined.clone());
                            taint.insert(dest_key, joined);
                        }
                        return new_mem;
                    }

                if rvalue.contains("Box::new") || rvalue.contains("vec::new") {
                    if let Some(_var) = &stmt.place {
                        // update taint for alloc
                    }
                } else if rvalue.contains("move") && !rvalue.contains("[") { // only move transferr the ownership, [move] array (no heap)
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
                } else if rvalue.contains("copy ") && rvalue.contains(" as *mut i32 (PtrToPtr)") {
                    // POINTER COPY semantics: dst and src will share the same allocation
                    let src_var = extract_copied_var(rvalue); 
                        if let Some(dest) = &stmt.place {
                            let src_key = full_local_name(&src_var);
                            let dest_key = full_local_name(dest);
                            // propagate the cell value from the source to the destination,
                            // which (per propagate_cell_value) will merge the allocations
                            new_mem.propagate_cell_value(&src_key, &dest_key);
                            
                            // propagate taint from src_key to dest_key:
                             let taint_state_src = taint.get(&src_key).cloned().unwrap_or_default();
                            taint.entry(dest_key).or_insert_with(HashSet::new).extend(taint_state_src);
                        }
                    
 // case:
/*
        rust::main::bb3",
      {
        "node_type": "Mir",
        "node_data": {
          "block_id": 3,
          "statements": [
            {
              "source_info": {
                "span": "/home/af/Documenti/a-phd/cargo_project_test/cstr_cargo/src/main.rs:39:41: 39:44 (#0)",
                "scope": "scope[2]"
              },
              "kind": "Assign",
              "details": "Assign((_7, copy _4 as *const i8 (PtrToPtr)))",
              "place": "Local(_7) [mutable]",
              "is_mutable": true,
              "rvalue": "copy _4 as *const i8 (PtrToPtr)"
            }
          ],
          "terminator": {
            "kind": "Call",
            "details": "Terminator { source_info: SourceInfo { span: /home/af/Documenti/a-phd/cargo_project_test/cstr_cargo/src/main.rs:39:26: 39:45 (#0), scope: scope[2] }, kind: _6 = std::ffi::CStr::from_ptr::<'_>(move _7) -> [return: bb4, unwind continue] }",
            "source_info": "/home/af/Documenti/a-phd/cargo_project_test/cstr_cargo/src/main.rs:39:26: 39:45 (#0)",
            "function_called": "std::ffi::CStr::from_ptr::<'_>",
            "arguments": [
              {
                "arg": "Local(_7) [mutable]",
                "is_mutable": true
              }
            ],
            "return_place": "_6",
            "return_target": "bb4",
            "unwind_target": "continue"
          }
        }
      }
    ],
*/
                }  else if rvalue.contains("copy ") && rvalue.contains(" as *const i8 (PtrToPtr)") {
                    // POINTER COPY semantics: dst and src will share the same allocation
                    let src_var = extract_copied_var(rvalue); 
                        if let Some(dest) = &stmt.place {
                            let src_key = full_local_name(&src_var);
                            let dest_key = full_local_name(dest);
                            // propagate the cell value from the source to the destination,
                            // which (per propagate_cell_value) will merge the allocations
                            new_mem.propagate_cell_value(&src_key, &dest_key);
                            
                            // propagate taint from src_key to dest_key:
                             let taint_state_src = taint.get(&src_key).cloned().unwrap_or_default();
                            taint.entry(dest_key).or_insert_with(HashSet::new).extend(taint_state_src);
                        }
                    }

                // eg "Assign((_30, copy _27 as *mut std::string::String (PtrToPtr)))"
                else if rvalue.contains("copy ") && rvalue.contains(" as *mut std::string::String (PtrToPtr)"){
                
                    let src_var = extract_copied_var(rvalue); 
                    if let Some(dest) = &stmt.place {
                        let src_key = full_local_name(&src_var);
                        let dest_key = full_local_name(dest);
                        // propagate the cell value from the source to the destination,
                        // which (per propagate_cell_value) will merge the allocations
                        new_mem.propagate_cell_value(&src_key, &dest_key);
                        
                        // propagate taint from src_key to dest_key:
                        let taint_state_src = taint.get(&src_key).cloned().unwrap_or_default();
                        taint.entry(dest_key).or_insert_with(HashSet::new).extend(taint_state_src);

                    }
                }

                else if rvalue.contains("&(*") {
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
                    // Fallback: evaluate the rvalue normally.
                    let v = eval_rvalue(&stmt.details, mem);
                    new_mem = update_state(new_mem, &full_local_name(var), v);
                }
            }
        }
        _ => { /* nisba, oth kinds remain unchanged */ }
    }
    new_mem
}



pub fn apply_mir_terminator(mem: &AbstractMemory,taint: &mut TaintStateMap,term: &MirTerminator) -> AbstractMemory {let mut new_mem = mem.clone();

    match term {
        MirTerminator::Call {details, function_called, arguments, return_place, ..} => {
            // HANDLE Result::expect on CString
            if function_called.contains("std::result::Result::<std::ffi::CString, std::ffi::NulError>::expect") 
            || function_called.contains("std::ffi::CStr::from_ptr::<'_>")
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
                let explicit_drop = is_explicit_mem_drop(function_called);
                let raw_pointer_drop = is_raw_pointer_mem_drop(function_called);

                let (ret_value, updated_mem) = transfer_call(&new_mem, details, return_place);
                new_mem = updated_mem;
                let full_ret_place = full_local_name(return_place);

                // std::mem::drop returns (), and transfer_call already applies
                // the heap effect to the dropped argument. Do not create/update
                // an abstract allocation for the call's return place.
                if !explicit_drop {
                    new_mem = update_state(new_mem, &full_ret_place, ret_value);
                }

                // UPDATE TAINT
                if function_called.contains("NOT_HANDLED") {
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
                    ||  INTO_RAW_REGEX.is_match(&function_called) 
                    || BOX_VEC_INTO_RAW_REGEX.is_match(&function_called)
                    || VEC_INTO_RAW_REGEX.is_match(&function_called)
                    || BOX_INTO_RAW_NODE_GENERIC_REGEX.is_match(&function_called)

                    //vec
                    || function_called.contains("std::boxed::Box::<std::vec::Vec<char>>::into_raw")
                    // MV option<F>
                    || function_called.contains("std::boxed::Box::<std::option::Option<F>>::into_raw")
                    // MV MSG SENDER
                    || function_called.contains("std::boxed::Box::<std::sync::mpsc::Sender<SegmentMessage>>::into_raw")
                    

                    // MEM FORGET
                    || function_called.contains("std::mem::forget::<std::boxed::Box<i32>>") || function_called.contains("std::mem::forget::<std::boxed::Box<u32>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<u8>>") || function_called.contains("std::mem::forget::<std::boxed::Box<i8>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<i16>>") || function_called.contains("std::mem::forget::<std::boxed::Box<u16>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<i64>>") || function_called.contains("std::mem::forget::<std::boxed::Box<u64>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<i128>>") || function_called.contains("std::mem::forget::<std::boxed::Box<u128>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<f32>>") || function_called.contains("std::mem::forget::<std::boxed::Box<f64>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<usize>>") || function_called.contains("std::mem::forget::<std::boxed::Box<isize>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<std::string::String>>")
                    || function_called.contains("std::mem::forget::<std::ffi::CString>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<std::ffi::CString>>")
                    || function_called.contains("std::mem::forget::<std::boxed::Box<&str>>")

                    || function_called.contains("std::mem::forget::<std::boxed::Box<bool>>") || function_called.contains("std::mem::forget::<std::boxed::Box<char>>")

                  //  MEM FORGET regex
                    ||  MEM_FORGET_REGEX.is_match(&function_called) 
                    || MEM_FORGET_VEC_REGEX.is_match(&function_called)
                    || MEM_FORGET_BOX_VEC_REGEX.is_match(&function_called)

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
// LLVM TRANSFER FUNCTIONS (super blueprint)
// ----------------------------------------------------------------------
fn transfer_llvm_node(llvm_node: &LlvmJsonNode, in_mem: &AbstractMemory, in_taint: &TaintStateMap) -> (AbstractMemory, TaintStateMap) {
    let mut current_mem = in_mem.clone();
    let current_taint = in_taint.clone();
    for stmt in &llvm_node.svf_statements {
        current_mem = apply_llvm_statement(&current_mem, stmt);
        // (update taint for LLVM statements)

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

    if let (Some(mir_var), Some(llvm_var)) = (&dummycall_node.mir_var, &dummycall_node.llvm_var) {
        let full_mir  = full_local_name(mir_var);
        let full_llvm = llvm_var.to_string();

        // unify allocations: merge the cell values (and allocation sets)
        current_mem.propagate_cell_value(&full_mir, &full_llvm);
        current_mem.propagate_cell_value(&full_llvm, &full_mir);

        // unify taint: combine any taint tags on both sides
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



pub fn fixed_point_analysis(icfg: &GlobalICFGOrdered) -> (AbstractState, TaintState) {

    let mut succs_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &icfg.icfg_edges {
        succs_map
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.destination.clone());
    }

    // 1) init worklist
    let mut abs_state   = AbstractState::default();
    let mut taint_state = TaintState::default();
    let mut worklist: BTreeSet<String> = BTreeSet::new();
    let mut call_stack: Vec<(String, AbstractMemory, TaintStateMap)> = Vec::new();
    let mut visit_order: Vec<String> = Vec::new();

    // entrypoint
    let entry = get_entrypoint();
    println!("entrypoint: {}", entry);

    {
        let node = get_node_by_id(icfg, &entry);
        let (m, t) = transfer_function(
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

        // 2.a) intraprocedural
        if let Some(succs) = succs_map.get(&current) {
            for succ in succs {
                let node = get_node_by_id(icfg, succ);
                let (new_mem, _new_taint) = transfer_function(&node, &curr_mem, &curr_taint);

                let old_mem   = abs_state.get(succ).unwrap_or_default();
                let old_taint = taint_state.get(succ).cloned().unwrap_or_default();

                let joined_mem = old_mem.union(&new_mem);
                let mut joined_taint = TaintStateMap::default();
                for (alloc, &val) in joined_mem.state.iter() {
                    for var in &alloc.set {
                        if matches!(val, CellValue::MV | CellValue::FREED) {
                            let tag = if val == CellValue::MV { "assign" } else { "free" }.to_string();
                            joined_taint.entry(var.clone()).or_default().insert(tag);
                        }
                    }
                }

                let first = !abs_state.state_map.contains_key(succ) && !taint_state.contains_key(succ);
                if first || joined_mem != old_mem || joined_taint != old_taint {
                    abs_state.insert(succ.clone(), joined_mem);
                    taint_state.insert(succ.clone(), joined_taint);
                    worklist.insert(succ.clone());
                }
            }
        }

        // 2.b) interprocedural
        if let GlobalICFGNode::Mir(ref bb) = get_node_by_id(icfg, &current) {
            // call
            if let Some(MirTerminator::Call { function_called, .. }) = &bb.terminator {
                if !function_called.contains("closure#") {
                    if let Some(dest) = succs_map
                        .get(&current)
                        .and_then(|s| s.iter().find(|dst| dst.starts_with("dummyCall")))
                    {
                        if let GlobalICFGNode::DummyCall(d) = get_node_by_id(icfg, dest) {
                            if d.is_internal.unwrap_or(false) {
                                call_stack.push((
                                    d.outgoing_edge.clone(),
                                    curr_mem.clone(),
                                    curr_taint.clone(),
                                ));
                                worklist.insert(dest.clone());
                                worklist.insert(format!("rust::{}::bb0", function_called));
                                continue;
                            }
                        }
                    }
                }
            }
            // return
            if let Some(MirTerminator::Return { .. }) = &bb.terminator {
                if let Some((ret_node, call_mem, call_taint)) = call_stack.pop() {
                    let callee_mem   = abs_state.get(&current).unwrap_or_default();
                    let callee_taint = taint_state.get(&current).cloned().unwrap_or_default();

                    let joined_mem = call_mem.union(&callee_mem);
                    let mut joined_taint = call_taint.clone();
                    for (var, tags) in callee_taint {
                        joined_taint.entry(var.clone())
                                        .and_modify(|e| e.extend(tags.clone()))
                                        .or_insert(tags);
                    }

                    abs_state.insert(ret_node.clone(), joined_mem);
                    taint_state.insert(ret_node.clone(), joined_taint);
                    worklist.insert(ret_node);
                }
            }
        }
    }
    if let Some(last) = visit_order.iter().rev().find(|n| !n.starts_with("dummy")) {
        set_last_visited(last.clone());
    }

    (abs_state, taint_state)
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
    Drop,
}

#[derive(Debug, Clone)]
struct VarInfo {
    llvm_free: usize,
    drop_free: usize,
    used: bool,
    use_span: Option<String>,
    free_span: Option<(String, FreeKind)>, // stores the first free span and its kind (LLVM or Drop)
}

impl VarInfo {
    fn new() -> Self {
        Self {
            llvm_free: 0,
            drop_free: 0,
            used: false,
            use_span: None,
            free_span: None,
        }
    }
 
    // if both LLVM and Drop frees are registered, return the maximum of the two,
    // otherwise return the sum.
    // this way an LLVM free + a Drop free produces effective_free == 1.
    fn effective_free(&self) -> usize {
        if self.llvm_free > 0 && self.drop_free > 0 {
            std::cmp::max(self.llvm_free, self.drop_free)
        } else {
            self.llvm_free + self.drop_free
        }
    }
}
// DETECTION OF MEMORY ISSUES
pub fn detect_mem_issues(icfg: &GlobalICFGOrdered, taint_states: &TaintState, abs_state: &AbstractState) -> (MultiSet, MultiSet, MultiSet) {

    // SETUP
    // SVF‐level: VarID -> Name normalized
    let mut svf_to_name = BTreeMap::new();
    let mut ir_to_name  = BTreeMap::new();
    let mut var_info    = BTreeMap::new();

    //////////////////////////////////////////////////////////////////////////////
    // variables freed in LLVM
    let mut llvm_warnings: Vec<(String, String)> = Vec::new();
    //////////////////////////////////////////////////////////////////////////////
    
    for (block, vars) in taint_states.iter() {
        for (var, markers) in vars.iter() {
            if markers.contains("assign") {
                let norm = normalize_name(var);
                println!("Tracking variable '{}' from block '{}' (markers: {:?})", norm, block, markers);
                var_info.entry(norm).or_insert(VarInfo::new());
            }
        }
    }

    // worklist with deterministic order
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut worklist: VecDeque<String> = icfg.ordered_nodes.iter()
        .map(|(id, _)| id.clone())
        .collect();

    // get "rust::main::bb0" 
    if let Some(pos) = worklist.iter().position(|id| id == "rust::main::bb0") {
        let main_node = worklist.remove(pos).unwrap();
        worklist.push_front(main_node);
    } else {
        println!("Main node not found (i.e.:'rust::main::bb0'); use first node as entry point");
    }

    // free-flow e union-find
    let mut free_flow_vars   = BTreeSet::new();
    let mut free_flow_parent = BTreeMap::new();
    let mut processed_llvm_free = BTreeSet::new();

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
                    // USE: search pattern "&(*"
                    if stmt.details.contains("&(*") {
                        if let Some(start_idx) = stmt.details.find("&(*") {
                            if let Some(end_idx) = stmt.details[start_idx..].find(")") {
                                let var_used = stmt.details[start_idx + 3..start_idx + end_idx].trim().to_string();
                                let var_used = normalize_name(&var_used);
                               // println!("Found use pattern: '{}' -> '{}'", stmt.details, var_used);
                                if let Some(info) = var_info.get_mut(&var_used) {
                                    info.used = true;
                                    if info.use_span.is_none() {
                                        info.use_span = Some(stmt.source_info.span.clone());
                                   //     println!("Marking '{}' as used at span: {}", var_used, stmt.source_info.span);
                                    }
                                } else {
                                    let alt = format!("Local({})", var_used);
                                    let alt = normalize_name(&alt);
                                    if let Some(info) = var_info.get_mut(&alt) {
                                        info.used = true;
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
                                    if info.use_span.is_none() {
                                        info.use_span = Some(stmt.source_info.span.clone());
                                       // println!("Marking '{}' as used at span: {}", var_used_norm, stmt.source_info.span);
                                    }
                                } else {
                                    // try with the "Local(...)" wrapper
                                    let alt = normalize_name(&format!("Local({})", var_used_norm));
                                    if let Some(info) = var_info.get_mut(&alt) {
                                        info.used = true;
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
                    // detect the MIR-generated borrows 'Assign((_4, &_2))' OR 'Assign((_7, &_2))'
                    else if stmt.details.starts_with("Assign(") && stmt.details.contains("(&") {
                        let raw_place = stmt.place.as_ref().unwrap();
                        let dst = normalize_name(raw_place);
                        // extract the content of borrow (&_<num>)
                        if let Some(start) = stmt.details.find("&_") {
                            if let Some(end) = stmt.details[start..].find(')') {
                                let src = normalize_name(&format!("Local(_{})", &stmt.details[start+1..start+1+end-1]));
                                // add both to var_info AND union-find
                                var_info.entry(dst.clone()).or_insert_with(VarInfo::new);
                                var_info.entry(src.clone()).or_insert_with(VarInfo::new);
                                free_flow_parent.entry(dst.clone()).or_insert(dst.clone());
                                free_flow_parent.entry(src.clone()).or_insert(src.clone());
                                union(&dst, &src, &mut free_flow_parent);
                                //   println!("Aliased borrow: '{}' <-> '{}'", src, dst);
                            }
                        }
                    }
                }

                if let Some(MirTerminator::Drop { details, source_info, .. }) = &mir_block.terminator {
                    if details.contains("drop(") {
                        ////////////////////////////////////////////////////////////////////////////////////////////////////
                        // SKIP DROP: DON'T CONSIDER THE DROP INSTRUCTION IF THERE IS AN UNWIND EDGE
                        // e.g. "unwind" edge in the ICFG
                        let skip_drop = icfg.icfg_edges.iter().any(|e| {
                            e.destination == node_id && matches!(e.label.as_deref(), Some(label) if label.contains("unwind"))});
                        if skip_drop {
                            println!("-> Skip Drop in '{}' because has an unwind incoming edge", node_id);
                        } else if details.contains("drop(") {
                            /////////////////////////////////////////////////////////////////////////////////////////////////
                            if let Some(start_idx) = details.find("drop(") {
                                if let Some(end_idx) = details[start_idx..].find(")") {
                                    let var_dropped = details[start_idx + 5..start_idx + end_idx].trim().to_string();
                                    let var_dropped = normalize_name(&var_dropped);
                                    println!("Found drop pattern: '{}' -> '{}'", details, var_dropped);
                                    if let Some(info) = var_info.get_mut(&var_dropped) {
                                        if info.drop_free == 0 {
                                            info.drop_free = 1;
                                            info.free_span = Some((source_info.clone(), FreeKind::Drop));
                                        } else {
                                            // if already registered a drop free, increment (double free if > 1)
                                            info.drop_free += 1;
                                        }
                                       // println!("Variable '{}' new drop free count: {}", var_dropped, info.drop_free);
                                    } else {
                                        let alt = format!("Local({})", var_dropped);
                                        let alt = normalize_name(&alt);
                                        if let Some(info) = var_info.get_mut(&alt) {
                                            if info.drop_free == 0 {
                                                info.drop_free = 1;
                                                info.free_span = Some((source_info.clone(), FreeKind::Drop));
                                            } else {
                                                info.drop_free += 1;
                                            }
                                            println!("Alternative '{}' new drop free count: {}", alt, info.drop_free);
                                        } else {
                                            println!("No tracked variable found per '{}' o '{}'", var_dropped, alt);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }



                if let Some(MirTerminator::Call {details, source_info, function_called, arguments, return_place, ..}) = &mir_block.terminator {   
                    // new alias case: multiple allocation on CString::from_raw
                    if function_called.contains("std::ffi::CString::from_raw")
                        || FROM_RAW_REGEX.is_match(&function_called)
                        || BOX_VEC_FROM_RAW_REGEX.is_match(&function_called)
                        || VEC_FROM_RAW_REGEX.is_match(&function_called)
                        || BOX_FROM_RAW_NODE_GENERIC_REGEX.is_match(&function_called)
                    {
                        /* get the mir names:
                           - src is the local having ptr, e.g. "_13"
                           - dst is new CString local, e.g. "_47" */
                        let src = normalize_name(&arguments[0].arg);
                        let dst = normalize_name(return_place);
                        // ENSURE both have VarInfo
                        var_info.entry(src.clone()).or_insert_with(VarInfo::new);
                        var_info.entry(dst.clone()).or_insert_with(VarInfo::new);
                        
                        // COPY existing info from src -> dst (keep src)
                        if let Some(info) = var_info.get(&src).cloned() {
                            var_info.insert(dst.clone(), info);
                        }
                        // UNION in free-flow: set union‐find sets for Name, then union 
                        free_flow_parent.entry(src.clone()).or_insert(src.clone());
                        free_flow_parent.entry(dst.clone()).or_insert(dst.clone());
                        union(&src, &dst, &mut free_flow_parent);

                        // handle also “Local(_47)”
                        let dst_local = format!("Local({})", dst);
                        let dst_local = normalize_name(&dst_local);
                        var_info.entry(dst_local.clone()).or_insert_with(VarInfo::new);
                        free_flow_parent.entry(dst_local.clone()).or_insert(dst_local.clone());
                        union(&dst, &dst_local, &mut free_flow_parent);
                        println!("Aliased via from_raw: '{}' -> '{}'", src, dst);
                    }

                    // EXPLICIT std::mem::drop FUNCTION CALL
                    // Use exactly the same semantics as the transfer function:
                    //   drop(*mut T / *const T) -> no heap free
                    //   drop(tracked owning value) -> one Drop free
                    if is_explicit_mem_drop(function_called)
                        && !is_raw_pointer_mem_drop(function_called)
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
                                            used: false,
                                            use_span: None,
                                            free_span: Some((
                                                source_info.clone(),
                                                FreeKind::Drop,
                                            )),
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
            
                    if let Some((num_str, _)) = llvm_var.split_once('@') {
                        if let Ok(svf_id) = num_str.parse::<usize>() {
                            svf_to_name.insert(svf_id, llvm_var.clone());
                        }
                    }
            
                    if !dummy_call.is_internal.unwrap_or(false) {
                        if let Some(info) = var_info.remove(&mir_var) {
                       //     println!("Transferring from '{}' to '{}' with free tracking", mir_var, llvm_var);
                            var_info.insert(llvm_var.clone(), info);
                            free_flow_vars.insert(llvm_var.clone());
                            free_flow_parent.insert(llvm_var.clone(), llvm_var.clone());
                            free_flow_vars.insert(mir_var.clone());
                            free_flow_parent.insert(mir_var.clone(), llvm_var.clone());
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
              //  println!("-> LLVM node (id: {}) con {} SVF statements", llvm_node.node_id, llvm_node.svf_statements.len());
                for stmt in &llvm_node.svf_statements {
                    // 1) PROPAGATE THE MAPPING varid to name
                    if let (Some(lhs), Some(rhs)) = (stmt.lhs_var_id, stmt.rhs_var_id) {
                        if let Some(name) = svf_to_name.get(&rhs).cloned() {
                            svf_to_name.insert(lhs, name.clone());
                        }
                    }
                    // 2) EXTRACT SVF varid X e IR id N 
                    if let Some(eq_pos) = stmt.stmt_info.find('=') {
                        let before_eq = &stmt.stmt_info[..eq_pos];
                        if let Some(percent_pos) = before_eq.rfind('%') {
                            let digits: String = before_eq[percent_pos + 1..]
                                .chars()
                                .take_while(|c| c.is_ascii_digit())
                                .collect();
                            if !digits.is_empty() {
                                if let Ok(ir_id) = digits.parse::<usize>() {
                                    if let Some(lhs) = stmt.lhs_var_id {
                                        if let Some(name) = svf_to_name.get(&lhs).cloned() {
                                            ir_to_name.insert(ir_id, name.clone());
                                          //  println!("Mapped IR-temp '%{}' → {}", ir_id, name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // 3) PROPAGATION LOGIC VAR_INFO + UNION-FIND
                    if let Some(rhs) = stmt.rhs_var_id {
                        let rhs_name = normalize_name(&rhs.to_string());
                        if var_info.contains_key(&rhs_name) {
                            if let Some(lhs) = stmt.lhs_var_id {
                                let lhs_name = normalize_name(&lhs.to_string());
                              //  println!("LLVM assignment: '{}' propaga {} -> {}", stmt.stmt_info, rhs_name, lhs_name);
                                // move VarInfo da rhs_name a lhs_name
                                if let Some(info) = var_info.remove(&rhs_name) {
                                    var_info.insert(lhs_name.clone(), info);
                                }
                                // update union-find if needed
                                if free_flow_vars.contains(&rhs_name)
                                    || free_flow_vars.contains(&lhs_name)
                                    || free_flow_parent.contains_key(&rhs_name)
                                    || free_flow_parent.contains_key(&lhs_name)
                                {
                                    free_flow_vars.insert(lhs_name.clone());
                                    union(&rhs_name, &lhs_name, &mut free_flow_parent);
                                }
                            }
                        }
                    }
                }
                
                // handling free calls in the llvm:
                if llvm_node.info.contains("@free(") && llvm_node.node_kind_string == "FunCallBlock" {
                 //   println!("LLVM node {} è una free call; risolvo via IR-map", llvm_node.node_id);

                    // 4) ESTRACTION N from "%N"
                    if let Some(p) = llvm_node.info.find('%') {
                        let rest = &llvm_node.info[p + 1..];
                        if let Some(len) = rest.find(|c: char| !c.is_ascii_digit()) {
                            if let Ok(ir_id) = rest[..len].parse::<usize>() {
                                // 4.1) first, try in ir_to_name
                                let mut var_name_opt = ir_to_name.get(&ir_id).cloned();
                                
                                // if it fails, use free_flow_vars
                                if var_name_opt.is_none() {
                                    var_name_opt = free_flow_vars
                                        .iter()
                                        .find(|v| var_info.contains_key(*v))
                                        .cloned();
                                }
                                if let Some(var_name) = var_name_opt {
                                    let rep = find(&var_name, &mut free_flow_parent);
                                    if !processed_llvm_free.contains(&rep) {
                                        if let Some(info) = var_info.get_mut(&var_name) {
                                            info.llvm_free += 1;
                                            info.free_span = Some((llvm_node.info.clone(), FreeKind::LLVM));
                                         //   println!("Incremented LLVM free count per '{}'", var_name);

                                            if info.drop_free == 0 {
                                                // no drop registered -> warning
                                                llvm_warnings.push((var_name.clone(), llvm_node.info.clone()));
                                            }
                                        }
                                        processed_llvm_free.insert(rep);
                                    } else {
                                       println!("Group '{}' già processato per LLVM free", rep);
                                    }
                                } else {
                                   println!("No variabile found at free call %{}", ir_id);
                                }
                            }
                        }
                    }
                }
            },
            GlobalICFGNode::DummyRet(dummy_ret) => {
                println!("-> DummyRet node '{}': transferring from LLVM var {:?} to MIR var {:?}", node_id, dummy_ret.llvm_var, dummy_ret.mir_var);
                if let (Some(mir_var), Some(llvm_var)) = (&dummy_ret.mir_var, &dummy_ret.llvm_var) {
                    let mir_var = normalize_name(mir_var);
                    let llvm_var = normalize_name(llvm_var);
                    if let Some(info) = var_info.remove(&llvm_var) {
                        println!("Transferring from '{}' to '{}'", llvm_var, mir_var);
                        var_info.insert(mir_var.clone(), info);
                        if free_flow_vars.remove(&llvm_var) {
                            free_flow_vars.insert(mir_var.clone());
                            union(&llvm_var, &mir_var, &mut free_flow_parent);
                        }
                    }
                }
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
        if let Some(alloc) = abs_state.get_allocation(var) {
            for candidate in alloc.set.iter() {
                let cn = normalize_name(candidate);
                if all_vars.contains(&cn) {
                    free_flow_parent.entry(var.clone()).or_insert(var.clone());
                    union(var, &cn, &mut free_flow_parent);
                }
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
                        group_contains_any(&group, &last_keys) || info.effective_free() > 0
                    } else {
                        info.effective_free() > 0
                    }
                });
            } else {
                println!("NO taint state found for the last mir node '{}'", last_mir_main_id);
            }
        }
    }
        // FINAL MERGE: from var_info -> alloc_info, mantaining use_span e (span, FreeKind)
        let mut alloc_info: HashMap<String,(
            usize,                    // free count
            bool,                     // used ?
            Option<String>,           // use_span
            Option<(String, FreeKind)> // free_span + kind
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
            .or_insert((0, false, None, None));
        entry.0 += fc;
        entry.1 = entry.1 || used;

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

        for (alloc_key, (fc, used, use_span_opt, free_span_opt)) in &alloc_info {
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
                    // case Drop MIR: compare spans
                    Some((free_span, FreeKind::Drop)) => {
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
            alloc_info.into_iter().map(|(k, (fc, used, use_span, free_span_opt))| {
                let free_span_str = free_span_opt.map(|(span, _kind)| span);
                (k, (fc, used, use_span, free_span_str))
            }).collect();

       
        print_final_report(&report_alloc_info, &use_after_free, &double_free, &never_free);

        // warning per free LLVM
        for (var, span) in llvm_warnings {
            println!("\u{1FAB2}  WARNING: variable '{}' was allocated in Rust and then freed in C (LLVM free) at `{}`", var, span);
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
    use super::{AbstractMemory, Allocation, CellValue};
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
}
