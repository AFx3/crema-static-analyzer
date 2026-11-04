// THIS FILE CONTAINS THE UTILS FUNCTIONS
use rustc_middle::mir::UnwindAction;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::io::Read;
use crate::structs::GlobalICFGOrdered;
use crate::dumpdot::dump_global_icfg_nodes_and_edges_to_dot;
use serde_json;


use std::error::Error;
use crate::utils::serde_json::Value;

// used in the icfg.rs to load the FFI functions from a json file
pub fn load_ffi_functions(file_path: &str) -> Result<std::collections::HashSet<String>, Box<dyn Error>> {
    let mut file = File::open(file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let v: Value = serde_json::from_str(&contents)?;
    let ffi_functions = v["ffi_functions"].as_array()
        .ok_or("Missing ffi_functions field")?
        .iter()
        .filter_map(|val| val.as_str().map(|s| s.to_string()))
        .collect();
    Ok(ffi_functions)
}

// dump the global ICFG nodes and edges to a DOT file (ordered icfg)
pub fn dump_dot_from_global_icfg(icfg_json_path: &str) {
    let mut file = match File::open(icfg_json_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error opening {}: {:?}", icfg_json_path, e);
            return;
        }
    };
    let mut contents = String::new();
    if let Err(e) = file.read_to_string(&mut contents) {
        eprintln!("Error reading {}: {:?}", icfg_json_path, e);
        return;
    }
    let global_icfg: GlobalICFGOrdered = match serde_json::from_str(&contents) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error parsing global ICFG JSON: {:?}", e);
            return;
        }
    };

    // dump dot icfg file with nodes and edges
    if let Err(e) = dump_global_icfg_nodes_and_edges_to_dot(&global_icfg, "global_icfg_nodes_edges.dot") {
        eprintln!("Error dumping global ICFG nodes/edges DOT file: {:?}", e);
    } else {
        println!("Global ICFG nodes/edges DOT file saved as global_icfg_nodes_edges.dot");
    }
}



pub fn unwind_action_to_string(action: &UnwindAction) -> String {
    match action {
        UnwindAction::Continue => "continue".to_string(),
        UnwindAction::Unreachable => "unreachable".to_string(),
        UnwindAction::Terminate(_) => "terminate".to_string(),
        UnwindAction::Cleanup(bb) => format!("cleanup({:?})", bb),
    }
}


// functioncompute a hash (as hex string) from any hashable value
// used to generate the dummyNode id hashing the incoming and outgoing basic block ids
pub fn compute_hash<T: Hash>(t: &T) -> String {
    let mut hasher = DefaultHasher::new();
    t.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}
