use std::error::Error;
use std::fs::File;
use std::io::Write;
use crate::structs::{GlobalICFGOrdered, GlobalICFGNode};

// returns a plain text label for a GlobalICFGNode
// for MIR nodes it builds the label as:
//  - block id (first line)
//  - one line per statement (formatted as “kind: details”)
//  - a terminator on its own line
fn get_node_label(node: &GlobalICFGNode) -> String {
    match node {
        GlobalICFGNode::Mir(bb) => {
            let mut label = format!("MIR bb{}\n", bb.block_id);
            for stmt in &bb.statements {
                label.push_str(&format!("{}: {}\n", stmt.kind, stmt.details));
            }
            if let Some(term) = &bb.terminator {
                label.push_str(&format!("Terminator: {:?}\n", term));
            }
            label
        }
        GlobalICFGNode::Llvm(llvm_node) => {
            format!("LLVM node {}: {}", llvm_node.node_id, llvm_node.info)
        }
        GlobalICFGNode::DummyCall(dummy) => {
            format!(
                "DummyCall: {} (id: {})\nmir_var: {}\nllvm_var: {}\nis_internal: {}",
                dummy.dummy_node_name,
                dummy.id,
                dummy.mir_var.as_deref().unwrap_or("None"),
                dummy.llvm_var.as_deref().unwrap_or("None"),
                dummy.is_internal.map(|b| if b { "true" } else { "false" }).unwrap_or("None")
            )
        }
        GlobalICFGNode::DummyRet(dummy) => {
            format!(
                "DummyRet: {} (id: {})\nmir_var: {}\nllvm_var: {}\nis_internal: {}",
                dummy.dummy_node_name,
                dummy.id,
                dummy.mir_var.as_deref().unwrap_or("None"),
                dummy.llvm_var.as_deref().unwrap_or("None"),
                dummy.is_internal.map(|b| if b { "true" } else { "false" }).unwrap_or("None")
            )
        }
    }
}


// escapes a node identifier 
fn escape_id(id: &str) -> String {
    // for node IDs only need to escape quotes and backslashes
    id.replace("\\", "\\\\").replace("\"", "\\\"")
}

// converts a plain text label into an HTML-like label for DOT
// It escapes HTML special characters and replaces newlines with <BR/>
fn convert_to_html_label(label: &str) -> String {
    label
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\n", "<BR/>")
}

// Dumps DOT representation of the global ICFG using HTML-like labels
// HTML labels let us safely include complex characters and use <BR/> for newlines
pub fn dump_global_icfg_nodes_and_edges_to_dot(global_icfg: &GlobalICFGOrdered, output_file: &str) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(output_file)?;
    writeln!(file, "digraph GlobalICFGNodesEdges {{")?;
    
    // set the default shape for all nodes to box.
    writeln!(file, "    node [shape=box];")?;
    
   // emit nodes
   for (node_id, node) in &global_icfg.ordered_nodes {
    let raw_label = get_node_label(node);
    let html_label = convert_to_html_label(&raw_label);
    let safe_node_id = escape_id(node_id);
    let raw_label_lower = raw_label; 
    
    // Determine extra attributes std::boxed::Box::<std::string::String>::new
    // for all nodes, if the raw label contains "free" or "drop", use green
    // otw, for MIR nodes check if the terminator contains one of the red markers
    let extra_attributes = if let GlobalICFGNode::Mir(_) = node {
        if raw_label_lower.contains("std::boxed::Box::<i32>::new") || raw_label_lower.contains("std::boxed::Box::<u32>::new") ||
            raw_label_lower.contains("std::boxed::Box::<u8>::new") || raw_label_lower.contains("std::boxed::Box::<i8>::new") ||
            raw_label_lower.contains("std::boxed::Box::<u64>::new") || raw_label_lower.contains("std::boxed::Box::<i64>::new") ||
            raw_label_lower.contains("std::boxed::Box::<u16>::new") || raw_label_lower.contains("std::boxed::Box::<i16>::new") ||
            raw_label_lower.contains("std::boxed::Box::<u128>::new") || raw_label_lower.contains("std::boxed::Box::<i128>::new") ||
            raw_label_lower.contains("std::boxed::Box::<bool>::new") || raw_label_lower.contains("std::boxed::Box::<char>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::string::String>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_void>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_char>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_int>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_uint>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_long>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_ulong>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_longlong>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_ulonglong>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_short>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_ushort>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_schar>::new") ||
            raw_label_lower.contains("std::boxed::Box::<std::ffi::c_uchar>::new") ||
            raw_label_lower.contains("std::boxed::Box::<f64>::new") || raw_label_lower.contains("std::boxed::Box::<f32>::new") ||
            raw_label_lower.contains("std::slice::<impl [i32]>::into_vec::<std::alloc::Global>") ||
            raw_label_lower.contains("std::slice::<impl [&str]>::into_vec::<std::alloc::Global>")||
            raw_label_lower.contains("std::boxed::Box::<std::string::String>::new") ||
            raw_label_lower.to_lowercase().contains("cstring as std::convert::from")||
            raw_label_lower.to_lowercase().contains("std::ffi::cstring::new::") ||
            raw_label_lower.to_lowercase().contains("std::boxed::box::<&str>::new") 

           {
            r#", style="filled", fillcolor="yellow""#
        } else if raw_label_lower.contains("drop") {
            r#", style="filled", fillcolor="green""#
        }else if raw_label_lower.contains("into_raw") || raw_label_lower.contains("std::mem::forget") {
            r#", style="filled", fillcolor="red""#

        } else {
            r#", style="filled", fillcolor="gray89""#
        }
    } else {
        // For non-MIR nodes, if they contain "free" 
        if raw_label_lower.contains("@free") && matches!(node, GlobalICFGNode::Llvm(llvm_node) if llvm_node.node_source_loc.contains("CallICFGNode: ")) {
            r#", style="filled", fillcolor="green""#
        } else {
            match node {
                GlobalICFGNode::Llvm(_) => r#", style="filled", fillcolor="turquoise""#,
                GlobalICFGNode::DummyCall(_) | GlobalICFGNode::DummyRet(_) => r#", style="filled", fillcolor="white""#,
                _ => "",
            }
        }
    };
    
    // Use HTML-like label syntax
    writeln!(file, "    \"{}\" [label=<{}>{}];", safe_node_id, html_label, extra_attributes)?;
}
    
        // legenda
        writeln!(file, "    legend [shape=box, margin=0, label=<")?;
        writeln!(file, "      <TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\" CELLPADDING=\"4\">")?;
        writeln!(file, "        <TR><TD COLSPAN=\"2\"><B>Info:</B></TD></TR>")?;
        writeln!(file, "        <TR><TD BGCOLOR=\"yellow\">&nbsp;&nbsp;&nbsp;</TD><TD>MIR allocation</TD></TR>")?;
        writeln!(file, "        <TR><TD BGCOLOR=\"red\">&nbsp;&nbsp;&nbsp;</TD><TD>Ownership forgotten</TD></TR>")?;
        writeln!(file, "        <TR><TD BGCOLOR=\"green\">&nbsp;&nbsp;&nbsp;</TD><TD>Drop/free</TD></TR>")?;
        writeln!(file, "        <TR><TD BGCOLOR=\"gray89\">&nbsp;&nbsp;&nbsp;</TD><TD>MIR node</TD></TR>")?;
        writeln!(file, "        <TR><TD BGCOLOR=\"turquoise\">&nbsp;&nbsp;&nbsp;</TD><TD>LLVM node</TD></TR>")?;
        writeln!(file, "        <TR><TD BGCOLOR=\"white\">&nbsp;&nbsp;&nbsp;</TD><TD>Dummy node</TD></TR>")?;
        writeln!(file, "      </TABLE>")?;
        writeln!(file, "    >];")?;


    // emit edges
    for edge in &global_icfg.icfg_edges {
        // for edge labels, still use plain text escaping
        let raw_label = edge.label.as_deref().unwrap_or("");
        let safe_edge_label = raw_label.replace("\"", "\\\"").replace("\n", "\\n");
        let safe_source = escape_id(&edge.source);
        let safe_destination = escape_id(&edge.destination);
        writeln!(file, "    \"{}\" -> \"{}\" [label=\"{}\"];", safe_source, safe_destination, safe_edge_label)?;
    }
    
    writeln!(file, "}}")?;
    Ok(())
}
