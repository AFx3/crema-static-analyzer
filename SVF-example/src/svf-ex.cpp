/*
This file contains a driver program that calls SVF to build the ICFG.
It first generates the PAG, then performs Andersen's analysis to build the call graph.
Finally, it builds the ICFG and traverses it to print the nodes and edges.
The icfg outputs are located in the 'output' directory, in DOT and JSON formats.
*/

#include "SVF-LLVM/LLVMUtil.h"
#include "SVF-LLVM/LLVMModule.h"
#include "Graphs/SVFG.h"
#include "WPA/Andersen.h"
#include "SVF-LLVM/SVFIRBuilder.h"
#include "Util/Options.h"
#include "Graphs/PTACallGraph.h"
#include "Graphs/VFG.h"
#include "Graphs/ICFGEdge.h"
#include "SVFIR/SVFIR.h"
#include "SVFIR/SVFFileSystem.h"
#include "SVFIR/SVFType.h"
#include "Graphs/GenericGraph.h"

// EFX1: inspect LLVM 16 contracts structurally and keep library inference
// isolated from the SVF analysis module.
#include "llvm/ADT/Triple.h"
#include "llvm/Analysis/TargetLibraryInfo.h"
#include "llvm/IR/Attributes.h"
#include "llvm/IR/InstrTypes.h"
#include "llvm/IR/Instructions.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/Verifier.h"
#include "llvm/IRReader/IRReader.h"
#include "llvm/Support/ModRef.h"
#include "llvm/Support/SourceMgr.h"
#include "llvm/Transforms/Utils/BuildLibCalls.h"
#include "llvm/Transforms/Utils/Cloning.h"
#include "llvm/Config/llvm-config.h"

#include <algorithm>
#include <cstdint>
#include <fstream>
#include <memory>
#include <optional>
#include <json/json.h> 
#include <iterator>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <vector>


using namespace llvm;
using namespace std;
using namespace SVF;

////////////////////////////////////////////////////////////////////////////////////////// EFX1 LLVM EFFECT / PTA EVIDENCE

static std::string efxModRefName(llvm::ModRefInfo mr) {
    switch (mr) {
        case llvm::ModRefInfo::NoModRef: return "none";
        case llvm::ModRefInfo::Ref: return "read";
        case llvm::ModRefInfo::Mod: return "write";
        case llvm::ModRefInfo::ModRef: return "readwrite";
    }
    return "readwrite"; // exhaustive defensive default for future enum changes
}

static Json::Value efxMemoryEffectsJson(const llvm::MemoryEffects& me) {
    Json::Value out;
    out["argmem"] = efxModRefName(me.getModRef(llvm::MemoryEffects::ArgMem));
    out["inaccessiblemem"] = efxModRefName(me.getModRef(llvm::MemoryEffects::InaccessibleMem));
    out["other"] = efxModRefName(me.getModRef(llvm::MemoryEffects::Other));
    out["encoded"] = static_cast<Json::UInt>(me.toIntValue());
    return out;
}

static Json::Value efxAllocKindJson(const llvm::Function& f) {
    Json::Value out(Json::arrayValue);
    if (!f.hasFnAttribute(llvm::Attribute::AllocKind)) return out;
    const auto bits = static_cast<uint64_t>(f.getFnAttribute(llvm::Attribute::AllocKind).getAllocKind());
    auto add = [&](llvm::AllocFnKind k, const char* name) {
        if (bits & static_cast<uint64_t>(k)) out.append(name);
    };
    add(llvm::AllocFnKind::Alloc, "alloc");
    add(llvm::AllocFnKind::Realloc, "realloc");
    add(llvm::AllocFnKind::Free, "free");
    add(llvm::AllocFnKind::Uninitialized, "uninitialized");
    add(llvm::AllocFnKind::Zeroed, "zeroed");
    add(llvm::AllocFnKind::Aligned, "aligned");
    return out;
}

static Json::Value efxFunctionSnapshot(const llvm::Function& f) {
    Json::Value out;
    out["nofree"] = f.hasFnAttribute(llvm::Attribute::NoFree);
    out["nosync"] = f.hasFnAttribute(llvm::Attribute::NoSync);
    out["willreturn"] = f.hasFnAttribute(llvm::Attribute::WillReturn);
    out["nobuiltin"] = f.hasFnAttribute(llvm::Attribute::NoBuiltin);
    out["optnone"] = f.hasFnAttribute(llvm::Attribute::OptimizeNone);
    out["memory_explicit"] = f.hasFnAttribute(llvm::Attribute::Memory);
    out["memory"] = efxMemoryEffectsJson(f.getMemoryEffects());
    out["alloc_kind"] = efxAllocKindJson(f);
    out["return_noalias"] = f.hasRetAttribute(llvm::Attribute::NoAlias);

    if (f.hasFnAttribute("alloc-family")) {
        const llvm::Attribute a = f.getFnAttribute("alloc-family");
        out["alloc_family"] = a.getValueAsString().str();
    } else {
        out["alloc_family"] = Json::Value(Json::nullValue);
    }

    if (f.hasFnAttribute(llvm::Attribute::AllocSize)) {
        const auto args = f.getFnAttribute(llvm::Attribute::AllocSize).getAllocSizeArgs();
        Json::Value as;
        as["element_size_arg"] = args.first;
        if (args.second) as["num_elements_arg"] = *args.second;
        else as["num_elements_arg"] = Json::Value(Json::nullValue);
        out["alloc_size"] = as;
    } else {
        out["alloc_size"] = Json::Value(Json::nullValue);
    }

    Json::Value formals(Json::arrayValue);
    for (unsigned i = 0; i < f.arg_size(); ++i) {
        const llvm::Argument* a = f.getArg(i);
        Json::Value formal;
        formal["index"] = i;
        formal["pointer_typed"] = a && a->getType()->isPointerTy();
        formal["nofree"] = f.hasParamAttribute(i, llvm::Attribute::NoFree);
        formal["nocapture"] = f.hasParamAttribute(i, llvm::Attribute::NoCapture);
        formal["returned"] = f.hasParamAttribute(i, llvm::Attribute::Returned);
        formal["readnone"] = f.hasParamAttribute(i, llvm::Attribute::ReadNone);
        formal["readonly"] = f.hasParamAttribute(i, llvm::Attribute::ReadOnly);
        formal["writeonly"] = f.hasParamAttribute(i, llvm::Attribute::WriteOnly);
        formal["allocptr"] = f.hasParamAttribute(i, llvm::Attribute::AllocatedPointer);
        formal["allocalign"] = f.hasParamAttribute(i, llvm::Attribute::AllocAlign);
        formals.append(formal);
    }
    out["formals"] = formals;
    return out;
}

static Json::Value efxCallsites(const llvm::Module& m) {
    Json::Value calls(Json::arrayValue);
    for (const llvm::Function& f : m) {
        unsigned ordinal = 0;
        for (const llvm::BasicBlock& bb : f) {
            for (const llvm::Instruction& inst : bb) {
                const auto* cb = llvm::dyn_cast<llvm::CallBase>(&inst);
                if (!cb) continue;
                Json::Value c;
                c["caller"] = f.getName().str();
                c["ordinal"] = ordinal++;
                c["direct"] = cb->getCalledFunction() != nullptr;
                if (const llvm::Function* callee = cb->getCalledFunction())
                    c["callee"] = callee->getName().str();
                else
                    c["callee"] = Json::Value(Json::nullValue);
                c["callsite_memory_explicit"] = cb->getAttributes().hasFnAttr(llvm::Attribute::Memory);
                c["effective_memory"] = efxMemoryEffectsJson(cb->getMemoryEffects());
                calls.append(c);
            }
        }
    }
    return calls;
}

// Produce two logically separate views from the exact input IR:
//   explicit_input_ir      : attributes that were present on input;
//   llvm16_tli_inferred    : result of LLVM's name+prototype TargetLibraryInfo
//                            inference on a clone, never on the SVF analysis module.
static bool exportLlvmMemoryEffectsSidecar(
    const std::vector<std::string>& moduleNames,
    const std::string& outputFileName
) {
    Json::Value root;
    root["schema"] = "llvm_memory_effects_v1";
    root["llvm_version"] = LLVM_VERSION_STRING;
    root["explicit_basis"] = "llvm16_explicit_input_ir_v1";
    root["tli_basis"] = "llvm16_tli_libfunc_attrs_v1";
    Json::Value modules(Json::arrayValue);

    bool ok = true;
    for (const std::string& moduleName : moduleNames) {
        llvm::LLVMContext ctx;
        llvm::SMDiagnostic diag;
        std::unique_ptr<llvm::Module> original = llvm::parseIRFile(moduleName, diag, ctx);
        if (!original) {
            errs() << "EFX1: failed to parse original module for effect evidence: " << moduleName << "\n";
            diag.print("svf-example", errs());
            ok = false;
            continue;
        }
        if (llvm::verifyModule(*original, &errs())) {
            errs() << "EFX1: refusing invalid input IR for effect evidence: " << moduleName << "\n";
            ok = false;
            continue;
        }

        std::unique_ptr<llvm::Module> inferred = llvm::CloneModule(*original);
        const llvm::Triple triple(original->getTargetTriple());
        llvm::TargetLibraryInfoImpl tliImpl(triple);

        for (llvm::Function& f : *inferred) {
            if (!f.isDeclaration() || f.hasFnAttribute(llvm::Attribute::OptimizeNone) || f.hasFnAttribute(llvm::Attribute::NoBuiltin))
                continue;
            // Mirror the TLI/BuildLibCalls portion of LLVM16 InferFunctionAttrs with a per-function TargetLibraryInfo
            // view (including target-specific no-builtin controls), but apply
            // it only to the cloned evidence module.
            llvm::TargetLibraryInfo functionTli(tliImpl, std::optional<const llvm::Function*>{&f});
            (void)llvm::inferNonMandatoryLibFuncAttrs(f, functionTli);
        }

        if (llvm::verifyModule(*inferred, &errs())) {
            errs() << "EFX1: TLI evidence clone failed LLVM verification: " << moduleName << "\n";
            ok = false;
            continue;
        }

        Json::Value moduleJson;
        moduleJson["input"] = std::filesystem::path(moduleName).filename().string();
        moduleJson["target_triple"] = original->getTargetTriple();
        moduleJson["input_ir_verified"] = true;
        moduleJson["tli_clone_verified"] = true;
        Json::Value functions(Json::arrayValue);

        for (const llvm::Function& f : *original) {
            Json::Value record;
            record["name"] = f.getName().str();
            record["is_declaration"] = f.isDeclaration();
            record["origin_explicit"] = "explicit_input_ir";
            const Json::Value explicitSnapshot = efxFunctionSnapshot(f);
            record["explicit"] = explicitSnapshot;

            llvm::TargetLibraryInfo functionTli(tliImpl, std::optional<const llvm::Function*>{&f});
            llvm::LibFunc lf;
            const bool inferenceEligible =
                f.isDeclaration() &&
                !f.hasFnAttribute(llvm::Attribute::OptimizeNone) &&
                !f.hasFnAttribute(llvm::Attribute::NoBuiltin);
            const bool recognized = inferenceEligible && functionTli.getLibFunc(f, lf) && functionTli.has(lf);
            record["tli_recognized"] = recognized;
            if (recognized) record["tli_libfunc"] = functionTli.getName(lf).str();
            else record["tli_libfunc"] = Json::Value(Json::nullValue);

            if (const llvm::Function* inf = inferred->getFunction(f.getName())) {
                record["origin_inferred"] = "llvm_tli_inferred";
                const Json::Value inferredSnapshot = efxFunctionSnapshot(*inf);
                record["tli_inferred"] = inferredSnapshot;
                // This flag is deliberately scoped to the evidence vocabulary
                // exported by llvm_memory_effects_v1. LLVM's libfunc inference
                // can also add unrelated attributes; those must not masquerade
                // as a memory/effect delta that CREMA/CQPL can consume.
                record["tli_changed"] = (explicitSnapshot != inferredSnapshot);
            }
            functions.append(record);
        }
        moduleJson["functions"] = functions;
        moduleJson["callsites_explicit"] = efxCallsites(*original);
        moduleJson["callsites_tli_inferred"] = efxCallsites(*inferred);
        modules.append(moduleJson);
    }

    root["modules"] = modules;
    std::ofstream file(outputFileName);
    if (!file.is_open()) return false;
    file << root.toStyledString();
    return ok;
}

static bool exportSvfPointsToSidecar(
    const SVF::SVFModule* svfModule,
    SVF::SVFIR* pag,
    SVF::Andersen* ander,
    const std::string& outputFileName
) {
    if (!svfModule || !pag || !ander) return false;
    Json::Value root;
    root["schema"] = "svf_solved_points_to_v1";
    root["analysis"] = "AndersenWaveDiff";
    root["semantics"] = "may";
    root["formal_mapping_schema"] = "svf_formal_arg_index_v1";
    Json::Value functions(Json::arrayValue);

    std::vector<const SVF::SVFFunction*> orderedFunctions;
    for (const SVF::SVFFunction* func : svfModule->getSVFModule()->getFunctionSet())
        if (func) orderedFunctions.push_back(func);
    std::sort(orderedFunctions.begin(), orderedFunctions.end(),
              [](const SVF::SVFFunction* lhs, const SVF::SVFFunction* rhs) {
                  if (lhs->getName() != rhs->getName())
                      return lhs->getName() < rhs->getName();
                  return lhs->arg_size() < rhs->arg_size();
              });

    for (const SVF::SVFFunction* func : orderedFunctions) {
        // Keep Bpta-R1 aligned with the producer-certified formal mapping.
        // SVFIRBuilder creates formal argument value nodes for body-backed
        // functions; declarations are library/summary boundaries and have no
        // per-function ICFG artifact in the current producer.
        if (func->getBasicBlockList().empty()) continue;
        Json::Value fj;
        fj["function"] = func->getName();
        Json::Value formals(Json::arrayValue);
        for (u32_t i = 0; i < func->arg_size(); ++i) {
            const SVF::SVFArgument* arg = func->getArg(i);
            if (!arg) continue;
            const SVF::NodeID varId = pag->getValueNode(arg);
            Json::Value formal;
            formal["formal_index"] = i;
            formal["svf_var_id"] = static_cast<Json::UInt64>(varId);
            Json::Value pts(Json::arrayValue);
            std::vector<SVF::NodeID> ids;
            const auto& solvedPts = ander->getPts(varId);
            for (auto it = solvedPts.begin(), e = solvedPts.end(); it != e; ++it)
                ids.push_back(*it);
            std::sort(ids.begin(), ids.end());
            ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
            for (SVF::NodeID id : ids) pts.append(static_cast<Json::UInt64>(id));
            formal["points_to"] = pts;
            formals.append(formal);
        }
        fj["formals"] = formals;
        functions.append(fj);
    }
    root["functions"] = functions;

    std::ofstream file(outputFileName);
    if (!file.is_open()) return false;
    file << root.toStyledString();
    return true;
}

////////////////////////////////////////////////////////////////////////////////////////// UTILS

std::string getEdgeKindAsString(ICFGEdge* edge) {
    switch (edge->getEdgeKind()) {
        case ICFGEdge::IntraCF:
            return "intra";
        case ICFGEdge::CallCF:
            return "call";
        case ICFGEdge::RetCF:
            return "ret";
        default:
            return "unknown";
    }
}

std::string getNodeKindString(int kind) {
    switch (kind) {
        case SVF::ICFGNode::IntraBlock: return "IntraBlock";
        case SVF::ICFGNode::FunEntryBlock: return "FunEntryBlock";
        case SVF::ICFGNode::FunExitBlock: return "FunExitBlock";
        case SVF::ICFGNode::FunCallBlock: return "FunCallBlock";
        case SVF::ICFGNode::FunRetBlock: return "FunRetBlock";
        case SVF::ICFGNode::ValNode: return "ValNode";
        default: return "UnknownKind";
    }
}

void writeNode(std::ofstream &dotFile, const ICFGNode* node) {
    if (node) {
        // more descriptive attributes for nodes
        dotFile << "  Node" << reinterpret_cast<std::uintptr_t>(node)
                << " [label=\"Node " << reinterpret_cast<std::uintptr_t>(node)
                << "\\n" << node->toString()
                << "\", shape=record, color=blue];\n"; // Adjusted for better visualization
    }
}

void writeEdge(std::ofstream &dotFile, const ICFGNode* from, const ICFGNode* to, const std::string& edgeLabel = "") {
    if (from && to) {
        dotFile << "  Node" << reinterpret_cast<std::uintptr_t>(from)
                << " -> Node" << reinterpret_cast<std::uintptr_t>(to);
        if (!edgeLabel.empty()) {
            dotFile << " [label=\"" << edgeLabel << "\"]";
        }
        dotFile << ";\n";
    }
}



////////////////////////////////////////////////////////////////////////////// TRAVERSE ICFG AND DUMP TO JSON
// traverse the ICFG and export to a JSON file
void traverseAndExportICFGToJson(ICFG* icfg, const ICFGNode* startNode, const std::string& outputFileName) {
    // create a JSON root object
    Json::Value root;
    // arrays to hold nodes and edges
    Json::Value nodesJson(Json::arrayValue);
    Json::Value edgesJson(Json::arrayValue);

    // worklist and visited set for BFS traversal
    FIFOWorkList<const ICFGNode*> worklist;
    Set<const ICFGNode*> visited;

    // start traversal from the start node
    worklist.push(startNode);
    visited.insert(startNode);  // set the start node as visited

    while (!worklist.empty()) {
        const ICFGNode* iNode = worklist.pop();
        
        // check if node is valid
        if (iNode) {
            // create a node JSON object
            Json::Value nodeJson;
            nodeJson["node_id"] = (uintptr_t)iNode;  // id based on node address
            nodeJson["node_type"] = iNode->getType(); // add node kind
            nodeJson["info"] = iNode->toString();   // additional node information (e.g., instruction details)
            nodeJson["node_kind"] = iNode->getNodeKind(); // add node kind



            // get associated function and add to node JSON
            const SVF::SVFFunction* func = iNode->getFun();
            if (func) {
                nodeJson["function"] = (uintptr_t)func;           // use address as unique identifier
                nodeJson["function_name"] = func->getName();      // function name
            } else {
                nodeJson["function"] = "None";
                nodeJson["function_name"] = "None";
            }

            // get associated basic block and add to node JSON
            const SVF::SVFBasicBlock* basicBlock = iNode->getBB();
            if (basicBlock) {
                nodeJson["basic_block"] = (uintptr_t)basicBlock;
                nodeJson["basic_block_name"] = basicBlock->getName();
                nodeJson["basic_block_info"] = basicBlock->toString();
            } else {
                nodeJson["basic_block"] = "None";
                nodeJson["basic_block_name"] = "None";
            }

            // add associated SVF statements to the node JSON
            const auto& svfStmts = iNode->getSVFStmts();
            Json::Value svfStmtsJson(Json::arrayValue);
            for (const auto* stmt : svfStmts) {
                svfStmtsJson.append((uintptr_t)stmt); // add the address of each SVF statement
            }
            nodeJson["svf_statements"] = svfStmtsJson;



             // asdd incoming and outgoing edges
            Json::Value incomingEdgesJson(Json::arrayValue);
            Json::Value outgoingEdgesJson(Json::arrayValue);
            for (auto it = iNode->InEdgeBegin(); it != iNode->InEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)(*it)->getSrcNode(); // source node
                edgeJson["destination"] = (uintptr_t)(*it)->getDstNode(); // destination node
                edgeJson["edge_type"] = getEdgeKindAsString(*it);   // edge type
                incomingEdgesJson.append(edgeJson);
            }
            for (auto it = iNode->OutEdgeBegin(); it != iNode->OutEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)(*it)->getSrcNode(); // source node
                edgeJson["destination"] = (uintptr_t)(*it)->getDstNode(); // destination node
                edgeJson["edge_type"] = getEdgeKindAsString(*it);   // edge type
                outgoingEdgesJson.append(edgeJson);

            }
            // append node to the nodes array
            nodesJson.append(nodeJson);

            // process outgoing edges of the node
            for (ICFGNode::const_iterator it = iNode->OutEdgeBegin(), eit = iNode->OutEdgeEnd(); it != eit; ++it) {
                ICFGEdge* edge = *it;
                ICFGNode* succNode = edge->getDstNode();

                // add edge information to edgesJson
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)iNode;           // source node id
                edgeJson["destination"] = (uintptr_t)succNode;   // destination node id
                edgeJson["edge_type"] = getEdgeKindAsString(edge); // use the helper function to get edge type
                edgesJson.append(edgeJson);

                // if the successor node has not been visited, add it to the worklist
                if (visited.find(succNode) == visited.end()) {
                    visited.insert(succNode); // mark as visited
                    worklist.push(succNode);  // add to the worklist for BFS traversal
                }
            }
        }
    }

    // add the nodes and edges to the root JSON object
    root["nodes"] = nodesJson;
    root["edges"] = edgesJson;

    // output the JSON to a file
    std::ofstream file(outputFileName);
    if (file.is_open()) {
        file << root.toStyledString();  // Write formatted JSON to the file
        file.close();  // Close the file
    } else {
        errs() << "Failed to open output file: " << outputFileName << "\n";
    }
}

///////////////////////////////// CCCCCC \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\    
// this has node and edges lists: edges info also within the nodes
void traverseAndDumpICFGFullList(ICFG* icfg, const ICFGNode* startNode, const std::string& outputFileName) {
    // create a JSON root object
    Json::Value root;
    // arrays to hold nodes and edges
    Json::Value nodesJson(Json::arrayValue);
    Json::Value edgesJson(Json::arrayValue);

    // worklist and visited set for BFS traversal
    std::queue<const ICFGNode*> worklist;
    std::set<const ICFGNode*> visited;

    // start traversal from the start node
    worklist.push(startNode);
    visited.insert(startNode);

    while (!worklist.empty()) {
        const ICFGNode* iNode = worklist.front();
        worklist.pop();

        if (iNode) {
            // create a node JSON object
            Json::Value nodeJson;
            nodeJson["node_id"] = (uintptr_t)iNode; // Unique identifier based on node address
            nodeJson["node_type"] = iNode->getType(); // Node type
            nodeJson["info"] = iNode->toString(); // Node information
            nodeJson["node_kind"] = iNode->getNodeKind(); // Node kind

            // associated SVF function
            const SVFFunction* func = iNode->getFun();
            if (func) {
                nodeJson["function"] = (uintptr_t)func;
                nodeJson["function_name"] = func->getName();
            }

            // associated BB
            const SVFBasicBlock* basicBlock = iNode->getBB();
            if (basicBlock) {
                nodeJson["basic_block"] = (uintptr_t)basicBlock;
                nodeJson["basicBlockName"] = basicBlock->getName();
                nodeJson["basic_block_info"] = basicBlock->toString();
                
            }

            // associated statements
            const auto& svfStmts = iNode->getSVFStmts();
            Json::Value svfStmtsJson(Json::arrayValue);
            for (const auto* stmt : svfStmts) {
                Json::Value stmtJson;
                stmtJson["stmt_id"] = (uintptr_t)stmt;
                 stmtJson["stmt_info"] = stmt->toString(); // statement information

                svfStmtsJson.append(stmtJson);
            }
            nodeJson["svf_statements"] = svfStmtsJson;


            // add incoming and outgoing edges
            Json::Value incomingEdgesJson(Json::arrayValue);
            Json::Value outgoingEdgesJson(Json::arrayValue);
            for (auto it = iNode->InEdgeBegin(); it != iNode->InEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)(*it)->getSrcNode();
                edgeJson["destination"] = (uintptr_t)(*it)->getDstNode();
                edgeJson["edge_type"] = getEdgeKindAsString(*it);
                incomingEdgesJson.append(edgeJson);
            }
            for (auto it = iNode->OutEdgeBegin(); it != iNode->OutEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)(*it)->getSrcNode();
                edgeJson["destination"] = (uintptr_t)(*it)->getDstNode();
                edgeJson["edge_type"] = getEdgeKindAsString(*it);
                outgoingEdgesJson.append(edgeJson);

            }
            nodeJson["incoming_edges"] = incomingEdgesJson;
            nodeJson["outgoing_edges"] = outgoingEdgesJson;

            // append node to the nodes array
            nodesJson.append(nodeJson);

            // process outgoing edges of the node
            for (ICFGNode::const_iterator it = iNode->OutEdgeBegin(), eit = iNode->OutEdgeEnd(); it != eit; ++it) {
                ICFGEdge* edge = *it;
                ICFGNode* succNode = edge->getDstNode();

                // add edge information to edgesJson
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)iNode;
                edgeJson["destination"] = (uintptr_t)succNode;
                edgeJson["edge_type"] = getEdgeKindAsString(edge);
                edgesJson.append(edgeJson);

                // if the successor node has not been visited, add it to the worklist
                if (visited.find(succNode) == visited.end()) {
                    visited.insert(succNode);
                    worklist.push(succNode);
                }
            }
        }
    }

    // add the nodes and edges to the root JSON object
    root["nodes"] = nodesJson;
    root["edges"] = edgesJson;

    // output the JSON to a file
    std::ofstream file(outputFileName);
    if (file.is_open()) {
        file << root.toStyledString();
        file.close();
    } else {
        errs() << "Failed to open output file: " << outputFileName << "\n";
    }
}

///////////////////// CCCCCC \\\\\\\\\\\\\\\\\\\\\\\\\ 


void traverseAndDumpICFGemptlyEdge(ICFG* icfg, const ICFGNode* startNode, const std::string& outputFileName) {
    // create a JSON root object
    Json::Value root;
    // arrays to hold nodes and edges
    Json::Value nodesJson(Json::arrayValue);
    Json::Value edgesJson(Json::arrayValue);

    // worklist and visited set for BFS traversal
    std::queue<const ICFGNode*> worklist;
    std::set<const ICFGNode*> visited;

    // start traversal from the provided start node
    worklist.push(startNode);
    visited.insert(startNode);

    while (!worklist.empty()) {
        const ICFGNode* iNode = worklist.front();
        worklist.pop();

        if (iNode) {
            // create a node JSON object
            Json::Value nodeJson;
            nodeJson["node_id"] = (uintptr_t)iNode; // Unique identifier based on node address
            nodeJson["node_type"] = iNode->getType(); // Node type
            nodeJson["info"] = iNode->toString(); // Node information
            nodeJson["node_kind"] = iNode->getNodeKind(); // Node kind

            // associated SVF function
            const SVFFunction* func = iNode->getFun();
            if (func) {
                nodeJson["function"] = (uintptr_t)func;
                nodeJson["function_name"] = func->getName();
            }

            // associated basic block
            const SVFBasicBlock* basicBlock = iNode->getBB();
            if (basicBlock) {
                nodeJson["basic_block"] = (uintptr_t)basicBlock;
                nodeJson["basicBlockName"] = basicBlock->getName();
                nodeJson["basic_block_info"] = basicBlock->toString();
            }

            // associated statements
            const auto& svfStmts = iNode->getSVFStmts();
            Json::Value svfStmtsJson(Json::arrayValue);
            for (const auto* stmt : svfStmts) {
                Json::Value stmtJson;
                stmtJson["stmt_id"] = (uintptr_t)stmt;
                stmtJson["stmt_info"] = stmt->toString(); // Statement information

                svfStmtsJson.append(stmtJson);
            }
            nodeJson["svf_statements"] = svfStmtsJson;

            // add incoming and outgoing edges
            Json::Value incomingEdgesJson(Json::arrayValue);
            Json::Value outgoingEdgesJson(Json::arrayValue);
            for (auto it = iNode->InEdgeBegin(); it != iNode->InEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)(*it)->getSrcNode();
                edgeJson["destination"] = (uintptr_t)(*it)->getDstNode();
                edgeJson["edge_type"] = getEdgeKindAsString(*it);
                incomingEdgesJson.append(edgeJson);
            }
            for (auto it = iNode->OutEdgeBegin(); it != iNode->OutEdgeEnd(); ++it) {
                ICFGEdge* edge = *it;
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)edge->getSrcNode();
                edgeJson["destination"] = (uintptr_t)edge->getDstNode();
                edgeJson["edge_type"] = getEdgeKindAsString(edge);
                outgoingEdgesJson.append(edgeJson);

                // check if the edge is a CallCFGEdge
                if (llvm::isa<CallCFGEdge>(edge)) {
                    const CallCFGEdge* callEdge = llvm::cast<CallCFGEdge>(edge);

                    // add call points
                    Json::Value funcCallsJson(Json::arrayValue);
                    const auto& callPEs = callEdge->getCallPEs();
                    for (const auto* callPE : callPEs) {
                        Json::Value callJson;
                        callJson["call_id"] = (uintptr_t)callPE; // Unique ID for the call point
                        callJson["call_info"] = callPE->toString(); // Info about the call
                        funcCallsJson.append(callJson);
                    }
                    edgeJson["function_calls"] = funcCallsJson;
                }

                // if the successor node has not been visited, add it to the worklist
                ICFGNode* succNode = edge->getDstNode();
                if (visited.find(succNode) == visited.end()) {
                    visited.insert(succNode);
                    worklist.push(succNode);
                }
            }
            nodeJson["incoming_edges"] = incomingEdgesJson;
            nodeJson["outgoing_edges"] = outgoingEdgesJson;

            // append node to the nodes array
            nodesJson.append(nodeJson);
        }
    }

    // add the nodes and edges to the root JSON object
    root["nodes"] = nodesJson;
    root["edges"] = edgesJson;

    // output the JSON to a file
    std::ofstream file(outputFileName);
    if (file.is_open()) {
        file << root.toStyledString();
        file.close();
    } else {
        errs() << "Failed to open output file: " << outputFileName << "\n";
    }
}



///////////////////////////////////////////////////////////////////////////// DOT ICFG GRAPH GENERATION
// write a node in Graphviz format
void writeNodeToDot(std::ofstream &dotFile, const ICFGNode* node) {
    if (node) {
        // You can customize the node ID format
        dotFile << "  Node" << reinterpret_cast<std::uintptr_t>(node) << " [label=\"Node " << reinterpret_cast<std::uintptr_t>(node) << "\\n";
        dotFile << node->toString() << "\"];\n";
    }
}

// write an edge in Graphviz format
void writeEdgeToDot(std::ofstream &dotFile, const ICFGNode* from, const ICFGNode* to) {
    if (from && to) {
        dotFile << "  Node" << reinterpret_cast<std::uintptr_t>(from) << " -> Node" << reinterpret_cast<std::uintptr_t>(to) << ";\n";
    }
}

// traverseAndPrintICFG to generate .dot file output
void traverseAndPrintICFGToDot(ICFG* icfg, const ICFGNode* startNode, const std::string& dotFileName) {
    // open file stream to write .dot file
    std::ofstream dotFile(dotFileName);
    if (!dotFile.is_open()) {
        errs() << "Failed to open dot file for writing.\n";
        return;
    }

    // start of the Graphviz representation
    dotFile << "digraph ICFG {\n";
    dotFile << "  node [shape=box];\n"; // CAN CUSTOM NODE SHAPE

    // worklist to perform BFS 
    FIFOWorkList<const ICFGNode*> worklist;  // FIFO queue
    Set<const ICFGNode*> visited;            // set of visited nodes
    worklist.push(startNode);                // push start node to the worklist

    // traversal loop
    while (!worklist.empty()) {
        const ICFGNode* currentNode = worklist.pop(); // pop the first element

        // print node info to .dot file
        if (currentNode) {
            writeNodeToDot(dotFile, currentNode);  // write node to dot file

            // visit each outgoing edge of the node
            for (ICFGNode::const_iterator it = currentNode->OutEdgeBegin(), eit = currentNode->OutEdgeEnd(); it != eit; ++it) {
                ICFGEdge* edge = *it;  // current edge
                ICFGNode* successorNode = edge->getDstNode();  // successor node

                // if not visited, add to the worklist and mark as visited
                if (visited.find(successorNode) == visited.end()) {
                    visited.insert(successorNode);
                    worklist.push(successorNode);

                    // write edge to dot file
                    writeEdgeToDot(dotFile, currentNode, successorNode);
                }
            }
        }
    }

    // end of the Graphviz representation
    dotFile << "}\n";
    dotFile.close();

    errs() << "ICFG .dot file generated: " << dotFileName << "\n";
}
////////////////////////////////////////////////////////////////////////////////////////


///////////////////////////////////////////////////////////////////////////// TRAVERSAL 

// INPUT: pointer to ICFG object (the entire icfg), a constant pointer to starting ICFGNode)
void traverseAndPrintICFG(ICFG* icfg, const ICFGNode* startNode){


    // worklist to perform BFS traversal
    FIFOWorkList<const ICFGNode*> worklist; // FIFO queue named worklist taking a pointet to ICFGNode object
    Set<const ICFGNode*> visited;           // set visited to store the ICFG nodes that have been visited
    worklist.push(startNode);               // push the start node to the worklist


    // TRAVESAL
    while (!worklist.empty())
    {
        const ICFGNode* iNode = worklist.pop(); // pop the first element from the worklist

        // before visit next node, PRINT CONTENT OF THE CURRENT iNode
        if(iNode){
            errs() << "Processing Node: " << iNode << "\n";
            errs() << "Node Info: " << iNode->toString() << "\n"; 

            // print the associated function
            const SVF::SVFFunction* func = iNode->getFun(); // get the function associated with the node
            if(func){
                errs() << "Function associated with node: " << func << "\n";
            } else {
                errs() << "No function associated with node\n";
            }
            // print associated bb 
            const SVF::SVFBasicBlock* basicBlock = iNode->getBB();
            if(basicBlock){
                errs() << "Basic block associated with node: " << basicBlock << "\n";
            } else {
                errs() << "No basic block associated with node\n";
            }
            // print associated SVF statements
            const auto& svfStmts = iNode->getSVFStmts();
            if (!svfStmts.empty())
            {
                errs() << "Associated SVF Statements:\n";
                for (const auto* stmt : svfStmts)
                {
                    errs() << "  - " << stmt << "\n";
                }
            }
        }

        // visit each outgoing edge of the node
        // OutEdgeBegin() and OutEdgeEnd() are ICFGNode class methods returning iterators to the range of outgoing edges of current node
        for (ICFGNode::const_iterator it = iNode->OutEdgeBegin(), eit = iNode->OutEdgeEnd(); it != eit; ++it)
        {
            ICFGEdge* edge = *it; // pointer to current edge
            ICFGNode* succNode = edge->getDstNode(); // successor of the currnt inode
            // if the successor node hasn't been visited, add it to the worklist
            if (visited.find(succNode) == visited.end())
            {
                visited.insert(succNode);
                worklist.push(succNode);
            }
        }
    }
}



void icfgToDotOnlyNodeAndEdges(SVF::ICFG* icfg, const SVF::ICFGNode* currentNode, const std::string& filename) {
    // Open the DOT file for writing
    std::ofstream dotFile(filename);
    if (!dotFile.is_open()) {
        std::cerr << "Failed to open file: " << filename << std::endl;
        return;
    }

    dotFile << "digraph ICFG {\n";

    // Track visited nodes to avoid infinite loops
    std::unordered_set<const SVF::ICFGNode*> visited;
    std::queue<const SVF::ICFGNode*> worklist;

    // Start from the current node
    visited.insert(currentNode);
    worklist.push(currentNode);

    while (!worklist.empty()) {
        const SVF::ICFGNode* node = worklist.front();
        worklist.pop();

        // Process outgoing edges
        for (auto it = node->OutEdgeBegin(); it != node->OutEdgeEnd(); ++it) {
            const SVF::ICFGEdge* edge = *it;
            const SVF::ICFGNode* srcNode = edge->getSrcNode();
            const SVF::ICFGNode* dstNode = edge->getDstNode();

            // Node identifiers
            auto srcID = srcNode->getId();
            auto dstID = dstNode->getId();

            // Determine the edge kind (e.g., conditional, call, return, etc.)
            auto edgeKind = edge->getEdgeKind();

            // Add edge to the DOT file with appropriate labels
            if (edge->isCallCFGEdge()) {
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\" [label=\"Call\"];\n";
            } else if (edge->isRetCFGEdge()) {
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\" [label=\"Return\"];\n";
            } else if (edge->isIntraCFGEdge()) {
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\" [label=\"Intra\"];\n";
            } else {
                // Default case: generic edge
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\";\n";
            }

            // Add unvisited destination nodes to the worklist
            if (visited.find(dstNode) == visited.end()) {
                visited.insert(dstNode);
                worklist.push(dstNode);
            }
        }
    }

    dotFile << "}\n";
    dotFile.close();

    std::cout << "DOT file generated: " << filename << std::endl;
}

void icfgToDot(SVF::ICFG* icfg, const SVF::ICFGNode* currentNode, const std::string& filename) {
    // Open the DOT file for writing
    std::ofstream dotFile(filename);
    if (!dotFile.is_open()) {
        std::cerr << "Failed to open file: " << filename << std::endl;
        return;
    }

    dotFile << "digraph ICFG {\n";

    // Track visited nodes to avoid infinite loops
    std::unordered_set<const SVF::ICFGNode*> visited;
    std::queue<const SVF::ICFGNode*> worklist;

    // Start from the current node
    visited.insert(currentNode);
    worklist.push(currentNode);

    while (!worklist.empty()) {
        const SVF::ICFGNode* node = worklist.front();
        worklist.pop();

        // Add node information to the DOT file
        auto nodeID = node->getId();
        auto nodeType = node->getType();
        auto nodeKind = node->getNodeKind();
        std::string nodeInfo = node->toString();

        // Optional: Get associated function information if available
        const SVF::SVFFunction* func = node->getFun();
        std::string funcName = "None";
        if (func) {
            funcName = func->getName();
        }

        // Optional: Get associated basic block information if available
        const SVF::SVFBasicBlock* basicBlock = node->getBB();
        std::string basicBlockName = "None";
        std::string basicBlockInfo = "None";
        if (basicBlock) {
            basicBlockName = basicBlock->getName();
            basicBlockInfo = basicBlock->toString();
        }

        // Optional: Get associated SVF statements if available
        const auto& svfStmts = node->getSVFStmts();
        std::string svfStatements = "None";
        if (!svfStmts.empty()) {
            svfStatements = "[";
            for (const auto* stmt : svfStmts) {
                svfStatements += std::to_string((uintptr_t)stmt) + ", ";
            }
            svfStatements += "]";
        }

        // Add node details to the DOT file as node label
        dotFile << "  \"" << nodeID << "\" [label=\"ID: " << nodeID
                << "\\nType: " << nodeType
                << "\\nKind: " << nodeKind
                << "\\nInfo: " << nodeInfo
                << "\\nFunction: " << funcName
                << "\\nBasic Block: " << basicBlockName
                << "\\nStatements: " << svfStatements
                << "\"];\n";

        // Process outgoing edges
        for (auto it = node->OutEdgeBegin(); it != node->OutEdgeEnd(); ++it) {
            const SVF::ICFGEdge* edge = *it;
            const SVF::ICFGNode* srcNode = edge->getSrcNode();
            const SVF::ICFGNode* dstNode = edge->getDstNode();

            // Node identifiers for edges
            auto srcID = srcNode->getId();
            auto dstID = dstNode->getId();

            // Determine the edge kind (e.g., conditional, call, return, etc.)
            auto edgeKind = edge->getEdgeKind();

            // Add edge to the DOT file with appropriate labels
            if (edge->isCallCFGEdge()) {
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\" [label=\"Call\"];\n";
            } else if (edge->isRetCFGEdge()) {
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\" [label=\"Return\"];\n";
            } else if (edge->isIntraCFGEdge()) {
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\" [label=\"Intra\"];\n";
            } else {
                // Default case: generic edge
                dotFile << "  \"" << srcID << "\" -> \"" << dstID << "\";\n";
            }

            // Add unvisited destination nodes to the worklist
            if (visited.find(dstNode) == visited.end()) {
                visited.insert(dstNode);
                worklist.push(dstNode);
            }
        }
    }

    dotFile << "}\n";
    dotFile.close();

    std::cout << "DOT file generated: " << filename << std::endl;
}
/*
//////////////////////////////////////////////////////////////////// FULL INFO ICFG JSON OUTPUT \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\
*/
void outputFinalICFGJson(
    SVF::ICFG* icfg,
    const SVF::ICFGNode* startNode,
    const std::string& outputFileName,
    SVF::SVFIR* pag
) {

    Json::Value root;
    Json::Value nodesJson(Json::arrayValue);
    Json::Value edgesJson(Json::arrayValue);
    
    std::queue<const SVF::ICFGNode*> worklist;
    std::set<const SVF::ICFGNode*> visited;
    
    worklist.push(startNode);
    visited.insert(startNode);

    
    while (!worklist.empty()) {
        const SVF::ICFGNode* iNode = worklist.front();
        worklist.pop();

        if (iNode) {
// NODE INFO
            Json::Value nodeJson;
            nodeJson["node_id"] = static_cast<Json::UInt64>(iNode->getId());
            nodeJson["node_type"] = iNode->getType();
            nodeJson["info"] = iNode->toString();
            nodeJson["node_kind"] = iNode->getNodeKind();
            nodeJson["node_source_loc"] = iNode->getSourceLoc();
            // Add node kind and string representation
            auto kind = iNode->getNodeKind();
            nodeJson["node_kind_string"] = getNodeKindString(static_cast<int>(kind));
            nodeJson["node_kind"] = iNode->getNodeKind();  // Store integer value
        
// FUNCTION INFO
            const SVF::SVFFunction* func = iNode->getFun();
            if (func) {
                                nodeJson["function_name"] = func->getName();
                
            }
// BASIC BLOCK INFO
            const SVF::SVFBasicBlock* basicBlock = iNode->getBB();
            if (basicBlock) {
                                nodeJson["basicBlockName"] = basicBlock->getName();
                nodeJson["basic_block_info"] = basicBlock->toString();
            }

// STATEMENTS INFO
            // N.B.: for my purposes, i really do not need to be fully loyal to the SVF::SVFStmt class hierarchy, so i take just what i need 
            Json::Value svfStmtsJson(Json::arrayValue);
            for (const auto* stmt : iNode->getSVFStmts()) {

            if (!stmt) continue;

            Json::Value stmtJson;
            stmtJson["stmt_id"] = static_cast<Json::UInt64>(stmt->getEdgeID());
            stmtJson["stmt_info"] = stmt->toString();
            stmtJson["edge_id"] = stmt->getEdgeID();
            stmtJson["pta_edge"] = stmt->isPTAEdge();

            // StoreStmt 
            if (const auto* storeStmt = llvm::dyn_cast<SVF::StoreStmt>(stmt)) {
                stmtJson["stmt_type"] = "StoreStmt";
                stmtJson["lhs_var_id"] = storeStmt->getLHSVarID();
                stmtJson["rhs_var_id"] = storeStmt->getRHSVarID();
            }
            // CmpStmt
            else if (const auto* cmpStmt = llvm::dyn_cast<SVF::CmpStmt>(stmt)) {
                stmtJson["stmt_type"] = "CmpStmt";
                stmtJson["predicate"] = cmpStmt->getPredicate();
                stmtJson["res_var_id"] = cmpStmt->getResID();

                Json::Value operandsJson(Json::arrayValue);
                for (u32_t i = 0; i < cmpStmt->getOpVarNum(); ++i) {
                    operandsJson.append(cmpStmt->getOpVarID(i));
                }
                stmtJson["operand_var_ids"] = operandsJson;
            }

            // PhiStmt 
            else if (const auto* phiStmt = llvm::dyn_cast<SVF::PhiStmt>(stmt)) {
                stmtJson["stmt_type"] = "PhiStmt";

                // Keep the historical fields for backward compatibility, but
                // also emit the canonical scalar/vector schema consumed by
                // CREMA Phase 5.  Existing archived JSON therefore remains
                // readable and newly generated JSON is self-consistent.
                const auto resultVar = phiStmt->getResID();
                stmtJson["res_var_id"] = resultVar;
                stmtJson["lhs_var_id"] = resultVar;

                Json::Value operandsJson(Json::arrayValue);
                Json::Value operandIdsJson(Json::arrayValue);
                for (u32_t i = 0; i < phiStmt->getOpVarNum(); ++i) {
                    const auto opVar = phiStmt->getOpVarID(i);
                    Json::Value operandJson;
                    operandJson["op_var_id"] = opVar;
                    operandJson["icfg_node"] = static_cast<Json::UInt64>(phiStmt->getOpICFGNode(i)->getId());
                    operandsJson.append(operandJson);
                    operandIdsJson.append(opVar);
                }
                stmtJson["operand_vars"] = operandsJson;
                stmtJson["operand_var_ids"] = operandIdsJson;
            }

            // BinaryOPStmt
            else if (const auto* binOpStmt = llvm::dyn_cast<SVF::BinaryOPStmt>(stmt)) {
                stmtJson["stmt_type"] = "BinaryOPStmt";
                stmtJson["opcode"] = binOpStmt->getOpcode();
                stmtJson["res_var_id"] = binOpStmt->getResID();
        
                Json::Value operandsJson(Json::arrayValue);
                for (u32_t i = 0; i < binOpStmt->getOpVarNum(); ++i) {
                    operandsJson.append(binOpStmt->getOpVarID(i));
                }
                stmtJson["operand_var_ids"] = operandsJson;
            }
            // UnaryOPStmt
            else if (const auto* unaryOpStmt = llvm::dyn_cast<SVF::UnaryOPStmt>(stmt)) {
                stmtJson["stmt_type"] = "UnaryOPStmt";
                stmtJson["opcode"] = unaryOpStmt->getOpcode();
                stmtJson["res_var_id"] = unaryOpStmt->getResID();
                stmtJson["operand_var_id"] = unaryOpStmt->getOpVarID();
            }
            // LoadStmt 
            // put load before assign to avoid misclassification: LoadStmt is subclass of AssignStmt: f stmt is a LoadStmt, it can also be cast as an AssignStmt
            else if (const auto* loadStmt = llvm::dyn_cast<SVF::LoadStmt>(stmt)) {
                stmtJson["stmt_type"] = "LoadStmt";
                stmtJson["lhs_var_id"] = loadStmt->getLHSVarID();
                stmtJson["rhs_var_id"] = loadStmt->getRHSVarID();
            }
             // AddrStmt (subclass of assign)
            else if (const auto* addrStmt = llvm::dyn_cast<SVF::AddrStmt>(stmt)) {
                stmtJson["stmt_type"] = "AddrStmt";
                stmtJson["lhs_var_id"] = addrStmt->getLHSVarID();
                stmtJson["rhs_var_id"] = addrStmt->getRHSVarID();
            }
           
            // AssignStmt: keep it to stay generic
            else if (const auto* assignStmt = llvm::dyn_cast<SVF::AssignStmt>(stmt)) {
                stmtJson["stmt_type"] = "AssignStmt";
                stmtJson["lhs_var_id"] = assignStmt->getLHSVarID();
                stmtJson["rhs_var_id"] = assignStmt->getRHSVarID();
            }
            
            // CallPE
            else if (const auto* callStmt = llvm::dyn_cast<SVF::CallPE>(stmt)) {
                stmtJson["stmt_type"] = "CallPE";
                const auto* callInst = callStmt->getCallInst();
            if (callInst) {
                stmtJson["call_inst"] = callInst->toString();
            }
            stmtJson["lhs_var_id"] = callStmt->getLHSVarID();
            stmtJson["rhs_var_id"] = callStmt->getRHSVarID();
            }
            // BranchStmt
            else if (const auto* branchStmt = llvm::dyn_cast<SVF::BranchStmt>(stmt)) {
                stmtJson["stmt_type"] = "BranchStmt";
                stmtJson["is_conditional"] = branchStmt->isConditional();
                if (branchStmt->isConditional()) {
                    stmtJson["condition_var_id"] = static_cast<Json::UInt64>(branchStmt->getCondition()->getId());
            }

            Json::Value successorsJson(Json::arrayValue);
            for (u32_t i = 0; i < branchStmt->getNumSuccessors(); ++i) {
                Json::Value succJson;
                succJson["successor_id"] = static_cast<Json::UInt64>(branchStmt->getSuccessor(i)->getId());
                succJson["condition_value"] = static_cast<Json::Value::Int64>(branchStmt->getSuccessorCondValue(i));
                successorsJson.append(succJson);
            }
                stmtJson["successors"] = successorsJson;
            }

            else {
                stmtJson["stmt_type"] = "UnknownStmt";
            }
            svfStmtsJson.append(stmtJson);
        }
        nodeJson["svf_statements"] = svfStmtsJson;
//////////////////////////////////////////////////////////////////
// EDGES INFO
            Json::Value incomingEdgesJson(Json::arrayValue);
            Json::Value outgoingEdgesJson(Json::arrayValue);
            
            for (auto it = iNode->InEdgeBegin(); it != iNode->InEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = static_cast<Json::UInt64>((*it)->getSrcNode()->getId());
                edgeJson["destination"] = static_cast<Json::UInt64>((*it)->getDstNode()->getId());
                edgeJson["edge_type"] = getEdgeKindAsString(*it);

                incomingEdgesJson.append(edgeJson);
            }
            for (auto it = iNode->OutEdgeBegin(); it != iNode->OutEdgeEnd(); ++it) {
                Json::Value edgeJson;
                edgeJson["source"] = static_cast<Json::UInt64>((*it)->getSrcNode()->getId());
                edgeJson["destination"] = static_cast<Json::UInt64>((*it)->getDstNode()->getId());
                edgeJson["edge_type"] = getEdgeKindAsString(*it);

                outgoingEdgesJson.append(edgeJson);
            }
            nodeJson["incoming_edges"] = incomingEdgesJson;
            nodeJson["outgoing_edges"] = outgoingEdgesJson;
            
            nodesJson.append(nodeJson);
            
            for (auto it = iNode->OutEdgeBegin(); it != iNode->OutEdgeEnd(); ++it) {
                SVF::ICFGNode* succNode = (*it)->getDstNode();
                
                Json::Value edgeJson;
                edgeJson["source"] = static_cast<Json::UInt64>(iNode->getId());
                edgeJson["destination"] = static_cast<Json::UInt64>(succNode->getId());
                edgeJson["edge_type"] = getEdgeKindAsString(*it);

                edgesJson.append(edgeJson);
                
                if (visited.find(succNode) == visited.end()) {
                    visited.insert(succNode);
                    worklist.push(succNode);
                }
            }
        }
    }
    
    root["nodes"] = nodesJson;
    root["edges"] = edgesJson;

    // Bmulti producer certificate. SVFFunction stores formal SVFArgument
    // objects in declaration order; export their canonical VarIDs so CREMA
    // never infers positional identity from alloca/store traversal order.
    Json::Value formalParamVarIds(Json::arrayValue);
    if (const SVF::SVFFunction* func = startNode ? startNode->getFun() : nullptr) {
        for (u32_t i = 0; i < func->arg_size(); ++i) {
            const SVF::SVFArgument* arg = func->getArg(i);
            if (arg && pag) {
                // SVFArgument is an SVFValue, not an SVFVar in this pinned SVF.
                // Resolve its canonical PAG/SVFIR NodeID through the same API
                // already used by the original SVF example for points-to queries.
                const SVF::NodeID formalVarId = pag->getValueNode(arg);
                formalParamVarIds.append(static_cast<Json::UInt64>(formalVarId));
            }
        }
    }
    root["formal_param_var_ids"] = formalParamVarIds;
    root["formal_param_mapping_schema"] = "svf_formal_arg_index_v1";
    
    std::ofstream file(outputFileName);
    if (file.is_open()) {
        file << root.toStyledString();
        file.close();
    } else {
        std::cerr << "Failed to open output file: " << outputFileName << std::endl;
    }
}


int main(int argc, char **argv) {
    ////////////////////////////////////////////////////////////////////////////////////////// SETUP
    // parse command-line options
    std::vector<std::string> moduleNameVec = OptionBase::parseOptions(
        argc, argv, "Whole Program Points-to Analysis", "[options] <input-bitcode...>"
    );

    // CREMA v6G isolates every analysis run. Establish the directory before
    // preprocessing so EFX1 can archive evidence from the exact input IR.
    const char* outputDirEnv = std::getenv("CREMA_SVF_OUTPUT_DIR");
    std::string outputDir = (outputDirEnv && *outputDirEnv) ? outputDirEnv : "./output";
    std::filesystem::create_directories(outputDir);
    if (!outputDir.empty() && outputDir.back() != '/') outputDir.push_back('/');

    if (!exportLlvmMemoryEffectsSidecar(
            moduleNameVec, outputDir + "LLVM_MEMORY_EFFECTS_V1.json")) {
        errs() << "EFX1 fatal: LLVM memory-effect evidence export was incomplete.\n";
        return 2;
    }

    // preprocess the LLVM IR (same as wpa does)
    LLVMModuleSet::preProcessBCs(moduleNameVec);
    // build the SVF module
    SVFModule* svfModule = LLVMModuleSet::buildSVFModule(moduleNameVec);

    // build PAG/SVFIR
    SVFIRBuilder builder(svfModule);
    SVFIR* pag = builder.build();
    // Dump the pag to a file 
    //SVFIRWriter::writeJsonToPath(pag, "pag.dot");
    ////////////////////////////////////////////////////////////////////////////////////////// ANDERSEN
    // perform andersen's analysis (wpa)
    Andersen* ander = AndersenWaveDiff::createAndersenWaveDiff(pag);
    // dump points-to statistics (wpa)
    ander->dumpStat();
    if (!exportSvfPointsToSidecar(
            svfModule, pag, ander, outputDir + "SVF_SOLVED_POINTS_TO_V1.json")) {
        errs() << "EFX1 fatal: solved Andersen MAY points-to export failed.\n";
        AndersenWaveDiff::releaseAndersenWaveDiff();
        SVFIR::releaseSVFIR();
        SVF::LLVMModuleSet::releaseLLVMModuleSet();
        llvm::llvm_shutdown();
        return 3;
    }
    ////////////////////////////////////////////////////////////////////////////////////////// CALL GRAPH
    // create and dump the call graph (wpa)
    PTACallGraph* callgraph = ander->getCallGraph();
    callgraph->dump("callgraph_initial.dot");
    ////////////////////////////////////////////////////////////////////////////////////////// ICFG
    ICFG* icfg = pag->getICFG();
    // want see the icfg

    // iterate over all functions in the SVF module
    for (const SVF::SVFFunction* func : svfModule->getSVFModule()->getFunctionSet()) {
        const std::string& funcName = func->getName();


        // find first icfg node for the current function
        const ICFGNode* firstInstNode = nullptr;
        for (const SVF::SVFBasicBlock* bb : func->getBasicBlockList()) {
            // get the list of ICFGNodes associated with the basic block
            const std::vector<const ICFGNode*>& icfgNodes = bb->getICFGNodeList();

            if (!icfgNodes.empty()) {
                firstInstNode = icfgNodes.front();  // take the first ICFGNode
                break;
            }
        }

        if (firstInstNode) {
            // generate output files for the current function
            std::string outputPrefix = outputDir + funcName + "_";
            traverseAndPrintICFG(icfg, firstInstNode);
            traverseAndExportICFGToJson(icfg, firstInstNode, outputPrefix + "raw_icfg_SVF.json");
            traverseAndDumpICFGemptlyEdge(icfg, firstInstNode, outputPrefix + "no_edge_list_icfg_SVF.json");
            traverseAndDumpICFGFullList(icfg, firstInstNode, outputPrefix + "full_icfg_SVF.json");
            /////////////////////////////////////////////////////////////////////////////////
            outputFinalICFGJson(icfg, firstInstNode, outputPrefix + "A_FINAL_ICFG.json", pag);   // FINAL JSON WITH STMNT INFOS
            ////////////////////////////////////////////////////////////////////////////////
            icfgToDot(icfg, firstInstNode, outputPrefix + "view_icfg_output_SVF.dot");
            icfgToDotOnlyNodeAndEdges(icfg, firstInstNode, outputPrefix + "node_and_edges_only_icfg_SVF.dot");

            errs() << "ICFG Traversal Complete for function: " << funcName << ".\n";
        } else {
            errs() << "No instructions found in function: " << funcName << ".\n";
        }
    }

   

    // clean up resources
    AndersenWaveDiff::releaseAndersenWaveDiff();
    SVFIR::releaseSVFIR();
    LLVMModuleSet::getLLVMModuleSet()->dumpModulesToFile(".svf.bc");
    SVF::LLVMModuleSet::releaseLLVMModuleSet();
    llvm::llvm_shutdown();

    return 0;
}




