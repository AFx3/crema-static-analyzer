#[warn(non_snake_case)]
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

// -----------------------
// RUST MIR
// -----------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirStatement {
    pub source_info: SourceInfoData,  
    pub kind: String,             
    pub details: String,          
    pub place: Option<String>,
    pub is_mutable: Option<bool>, // if the place is mutable
    pub rvalue: Option<String>,   
}
// new struct to represent a call argument and its mutability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirCallArgument {
    pub arg: String,
    // if the argument comes from a place, this flag indicates whether that place is mutable.
    // (for non-place arguments, this will be None)
    pub is_mutable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfoData {
    pub span: String, 
    pub scope: String, 
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MirTerminator {
    Goto { details: String, source_info: String, target: String }, // 1
    SwitchInt { details: String, source_info: String, targets: Vec<String>, discr: String, otherwise: Option<String> }, //2
    UnwindResume { details: String, source_info: String }, //3
    Return { details: String, source_info: String }, //5
    //4 Abort {details: String, source_info: String}, //4 -- it's a  CALL ALLA process::abort() Not KIND TERMINATOR as 1.86
    Unreachable { details: String, source_info: String }, //6
    Drop { details: String, source_info: String, return_target: String, unwind_target: String, dropped_value: String, is_mutable: bool }, //7
    //Call { details: String, source_info: String, function_called: String, arguments: Vec<String>, return_place: String, return_target: Option<String>, unwind_target: String }, //8
    // --- modified Call variant: note that arguments is now a Vec<MirCallArgument> ---
    Call { details: String, source_info: String, function_called: String, arguments: Vec<MirCallArgument>, return_place: String, return_target: Option<String>, unwind_target: String },
    Assert { details: String, source_info: String, return_target: String, unwind_target: String, cond: String, expected: bool, msg: String }, //9
    InlineAsm { details: String, source_info: String, template: Vec<String>, operands: Vec<String>, options: String, line_spans: Vec<String>, unwind_target: Option<String> }, //10
    Unhandled { details: String, source_info: String },        
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirBasicBlock {
    pub block_id: usize,
    pub statements: Vec<MirStatement>,
    pub terminator: Option<MirTerminator>,
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirRepresentation {
    pub functions: HashMap<String, Vec<MirBasicBlock>>, // Function name -> Basic blocks
}
//whole mir as a mapping from function names to their corresponding basic blocks.

// -----------------------
// C LLVM IR 
// -----------------------

//(actually the svf icfg after Andersen's pta readen a json file)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlvmJsonNode {
    pub node_id: usize,
    pub node_type: bool,
    pub info: String,
    pub node_kind_string: String,
    pub node_kind: usize,
    pub node_source_loc: String,
    pub function_name: Option<String>,
    pub basic_block: Option<usize>,
    pub basic_block_name: Option<String>,
    pub basic_block_info: Option<String>,
    pub svf_statements: Vec<SvfStatement>,
    pub incoming_edges: Vec<LlvmEdge>,
    pub outgoing_edges: Vec<LlvmEdge>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvfPhiOperand {
    pub op_var_id: usize,
    /// ICFG node associated with this incoming Phi operand in the legacy
    /// SVF exporter schema.  It is not required for provenance propagation,
    /// but retaining it makes deserialization lossless for existing artifacts.
    pub icfg_node: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvfStatement {
    pub stmt_id: usize,
    pub stmt_type: String,
    pub stmt_info: String,
    pub edge_id: Option<usize>,
    pub pta_edge: Option<bool>,
    pub lhs_var_id: Option<usize>,
    pub rhs_var_id: Option<usize>,
    pub res_var_id: Option<usize>,
    pub operand_var_ids: Option<Vec<usize>>,
    /// Backward-compatible representation emitted by the pre-Phase-5
    /// `svf-ex.cpp` for PhiStmt.
    pub operand_vars: Option<Vec<SvfPhiOperand>>,
    pub call_inst: Option<String>,
    pub is_conditional: Option<bool>,
    pub condition_var_id: Option<usize>,
    pub successors: Option<Vec<BranchSuccessor>>,
}

impl SvfStatement {
    /// Canonical SSA result variable for statements whose producer schemas
    /// historically used either `lhs_var_id` or `res_var_id`.
    ///
    /// The Phase-5 consumer must accept both because existing SVF artifacts
    /// encode PhiStmt results as `res_var_id`, while the canonical exporter
    /// added by Phase 5 also emits `lhs_var_id`.
    pub fn result_var_id(&self) -> Option<usize> {
        self.lhs_var_id.or(self.res_var_id)
    }

    /// Canonical ordered list of operand VarIDs across both supported SVF
    /// JSON schemas. The canonical `operand_var_ids` field has precedence;
    /// `operand_vars[*].op_var_id` is a backward-compatibility fallback only.
    /// Duplicate IDs are removed without changing first-seen order.
    pub fn normalized_operand_var_ids(&self) -> Vec<usize> {
        let mut out = Vec::new();

        if let Some(ids) = &self.operand_var_ids {
            for id in ids {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            return out;
        }

        if let Some(operands) = &self.operand_vars {
            for operand in operands {
                if !out.contains(&operand.op_var_id) {
                    out.push(operand.op_var_id);
                }
            }
        }

        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchSuccessor {
    pub successor_id: usize, // ID of successor node
    pub condition_value: Option<i64>, // Condition value (if applicable)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlvmEdge {
    pub source: usize,
    pub destination: usize,
    pub edge_type: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlvmJson {
    pub nodes: Vec<LlvmJsonNode>, // list of nodes in the JSON structure
    pub edges: Vec<LlvmEdge>, // list of edges in the JSON structure
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlvmFunction {
    pub function_name: String,
    //pub basic_blocks: Vec<LlvmBasicBlock>,
    pub nodes: Vec<LlvmJsonNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlvmRepresentation {
    pub functions: HashMap<String, LlvmFunction>,
    pub global_edges: Vec<LlvmEdge>,
}

// -----------------------
// GLOBAL ICFG 
// -----------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcfgEdge {
    pub source: String,
    pub destination: String,
    pub label: Option<String>,
    pub source_label: Option<String>,
    pub destination_label: Option<String>,
}

// a node in the global ICFG can be either a MIR basic block, an LLVM node, or a dummy node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type", content = "node_data")]
pub enum GlobalICFGNode {
    Llvm(LlvmJsonNode),
    Mir(MirBasicBlock),
    DummyCall(DummyNode),
    DummyRet(DummyNode),
}

// a dummy node with extra fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DummyNode {
    pub dummy_node_name: String,
    pub incoming_edge: String,
    pub outgoing_edge: String,
    pub id: String,
    pub mir_var: Option<String>,
    pub llvm_var: Option<String>,
    pub is_internal: Option<bool>,
}

//  The overall global ICFG has a mapping from node IDs (strings) to optional node data
/// (if the node is found and is not a leaf, otherwise `None`), and a list of ICFG edges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalICFG {
    pub icfg_nodes: HashMap<String, Option<GlobalICFGNode>>,
    pub icfg_edges: Vec<IcfgEdge>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalICFGOrdered {
    pub ordered_nodes: Vec<(String, GlobalICFGNode)>,
    pub icfg_edges: Vec<IcfgEdge>,
}

