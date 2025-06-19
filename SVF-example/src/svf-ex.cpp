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

#include <fstream>
#include <json/json.h> 
#include <iterator>


using namespace llvm;
using namespace std;
using namespace SVF;

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
void outputFinalICFGJson(SVF::ICFG* icfg, const SVF::ICFGNode* startNode, const std::string& outputFileName) {

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
            nodeJson["node_id"] = (uintptr_t)iNode;
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
                nodeJson["function"] = (uintptr_t)func;
                nodeJson["function_name"] = func->getName();
                
            }
// BASIC BLOCK INFO
            const SVF::SVFBasicBlock* basicBlock = iNode->getBB();
            if (basicBlock) {
                nodeJson["basic_block"] = (uintptr_t)basicBlock;
                nodeJson["basicBlockName"] = basicBlock->getName();
                nodeJson["basic_block_info"] = basicBlock->toString();
            }

// STATEMENTS INFO
            // N.B.: for my purposes, i really do not need to be fully loyal to the SVF::SVFStmt class hierarchy, so i take just what i need 
            Json::Value svfStmtsJson(Json::arrayValue);
            for (const auto* stmt : iNode->getSVFStmts()) {

            if (!stmt) continue;

            Json::Value stmtJson;
            stmtJson["stmt_id"] = (uintptr_t)stmt;
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
                stmtJson["res_var_id"] = phiStmt->getResID();

                Json::Value operandsJson(Json::arrayValue);
                for (u32_t i = 0; i < phiStmt->getOpVarNum(); ++i) {
                    Json::Value operandJson;
                    operandJson["op_var_id"] = phiStmt->getOpVarID(i);
                    operandJson["icfg_node"] = (uintptr_t)phiStmt->getOpICFGNode(i);
                    operandsJson.append(operandJson);
                }
                    stmtJson["operand_vars"] = operandsJson;
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
                    stmtJson["condition_var_id"] = (uintptr_t)branchStmt->getCondition();
            }

            Json::Value successorsJson(Json::arrayValue);
            for (u32_t i = 0; i < branchStmt->getNumSuccessors(); ++i) {
                Json::Value succJson;
                succJson["successor_id"] = (uintptr_t)branchStmt->getSuccessor(i);
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
            
            nodesJson.append(nodeJson);
            
            for (auto it = iNode->OutEdgeBegin(); it != iNode->OutEdgeEnd(); ++it) {
                SVF::ICFGNode* succNode = (*it)->getDstNode();
                
                Json::Value edgeJson;
                edgeJson["source"] = (uintptr_t)iNode;
                edgeJson["destination"] = (uintptr_t)succNode;
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
            std::string outputPrefix = "./output/" + funcName + "_";
            traverseAndPrintICFG(icfg, firstInstNode);
            traverseAndExportICFGToJson(icfg, firstInstNode, outputPrefix + "raw_icfg_SVF.json");
            traverseAndDumpICFGemptlyEdge(icfg, firstInstNode, outputPrefix + "no_edge_list_icfg_SVF.json");
            traverseAndDumpICFGFullList(icfg, firstInstNode, outputPrefix + "full_icfg_SVF.json");
            /////////////////////////////////////////////////////////////////////////////////
            outputFinalICFGJson(icfg, firstInstNode, outputPrefix + "A_FINAL_ICFG.json");   // FINAL JSON WITH STMNT INFOS
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




