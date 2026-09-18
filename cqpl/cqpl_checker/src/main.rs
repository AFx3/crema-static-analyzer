use cqpl_checker::{
    panic_lifecycle_overlay_from_json, parse_query_document, AnnotatedIcfg, Binding, Env, Kripke,
    ModelChecker, QueryResultAssessment,
};
use serde::Serialize;
use serde_json::Value;
use std::{env, fs, path::Path, process};

#[derive(Serialize)]
struct JsonOutput<'a> {
    result: &'a str,
    assessment: &'a QueryResultAssessment,
    entry: &'a str,
    scope: &'a str,
    query_file: &'a str,
}

fn usage() -> ! {
    eprintln!("Usage: cqpl_checker <annotated-icfg.json> <query.cqpl> [--entry NODE_OR_FUNCTION] [--intra] [--bind x=PROGRAM_VAR_ID]... [--bind-alloc a=ABSTRACT_ALLOC_ID]... [--json] [--explain-json PATH] [--explain-unk-verbose] [--explain-max-witnesses N]");
    process::exit(2);
}

fn main() {
    if let Err(e) = run() {
        eprintln!("cqpl_checker: {e}");
        process::exit(2);
    }
}


fn validate_contract_object(value: &Value, where_: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{where_} must be an object"))?;
    for required in ["family", "operation", "language"] {
        if !object.get(required).is_some_and(Value::is_string) {
            return Err(format!("{where_} requires string field '{required}'"));
        }
    }
    let family = object["family"].as_str().unwrap();
    if !matches!(family, "rust_global" | "c_malloc" | "unknown") {
        return Err(format!("{where_} has unsupported allocator family '{family}'"));
    }
    let language = object["language"].as_str().unwrap();
    if !matches!(language, "rust" | "c" | "unknown") {
        return Err(format!("{where_} has unsupported language '{language}'"));
    }
    if object["operation"].as_str().unwrap().is_empty() {
        return Err(format!("{where_}.operation must be non-empty"));
    }
    Ok(())
}


fn validate_v2_deallocator_contract_object(value: &Value, where_: &str, allow_v3: bool) -> Result<(), String> {
    validate_contract_object(value, where_)?;
    let object = value.as_object().unwrap();
    let basis = object
        .get("basis")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{where_} requires non-empty string field 'basis' under allocation_contracts_v2"))?;
    let family = object["family"].as_str().unwrap();
    let operation = object["operation"].as_str().unwrap();
    let language = object["language"].as_str().unwrap();
    let owner = object.get("owner_def_path").and_then(Value::as_str);
    let allocator = object.get("allocator_def_path").and_then(Value::as_str);
    let callee = object.get("callee_def_path").and_then(Value::as_str);

    match basis {
        "rust_box_global_drop" | "rust_vec_global_drop" | "rust_cstring_global_drop" => {
            if (family, operation, language) != ("rust_global", "drop", "rust") {
                return Err(format!(
                    "{where_} basis '{basis}' requires family=rust_global operation=drop language=rust"
                ));
            }
            if owner.map_or(true, str::is_empty) || allocator.map_or(true, str::is_empty) {
                return Err(format!(
                    "{where_} basis '{basis}' requires non-empty owner_def_path and allocator_def_path"
                ));
            }
            if callee.is_some() {
                return Err(format!("{where_} basis '{basis}' does not accept callee_def_path"));
            }
        }
        "rust_global_dealloc_api" => {
            if (family, operation, language) != ("rust_global", "dealloc", "rust") {
                return Err(format!(
                    "{where_} basis '{basis}' requires family=rust_global operation=dealloc language=rust"
                ));
            }
            if owner.is_some() || allocator.is_some() {
                return Err(format!("{where_} basis '{basis}' does not accept typed-drop provenance fields"));
            }
            if callee.map_or(true, str::is_empty) {
                return Err(format!("{where_} basis '{basis}' requires non-empty callee_def_path"));
            }
        }
        "rust_mem_drop_owned_box_global_v1" => {
            if !allow_v3 {
                return Err(format!(
                    "{where_} basis 'rust_mem_drop_owned_box_global_v1' requires artifact capability allocation_contracts_v3"
                ));
            }
            if (family, operation, language) != ("rust_global", "drop", "rust") {
                return Err(format!(
                    "{where_} basis 'rust_mem_drop_owned_box_global_v1' requires family=rust_global operation=drop language=rust"
                ));
            }
            if owner.map_or(true, str::is_empty)
                || allocator.map_or(true, str::is_empty)
                || callee.map_or(true, str::is_empty)
            {
                return Err(format!(
                    "{where_} basis 'rust_mem_drop_owned_box_global_v1' requires non-empty owner_def_path, allocator_def_path, and callee_def_path"
                ));
            }
        }
        "structural_c_free_v1" => {
            if (family, operation, language) != ("c_malloc", "free", "c") {
                return Err(format!(
                    "{where_} basis '{basis}' requires family=c_malloc operation=free language=c"
                ));
            }
            if owner.is_some() || allocator.is_some() || callee.is_some() {
                return Err(format!("{where_} basis '{basis}' does not accept Rust provenance fields"));
            }
        }
        "unresolved" => {
            if family != "unknown" {
                return Err(format!("{where_} basis 'unresolved' must remain family=unknown"));
            }
            if owner.is_some() || allocator.is_some() || callee.is_some() {
                return Err(format!("{where_} unresolved contracts must not carry provenance fields"));
            }
        }
        other => {
            return Err(format!(
                "{where_} has unsupported allocation_contracts_v2 basis '{other}'"
            ));
        }
    }
    Ok(())
}

fn validate_disposition_object(
    value: &Value,
    known_allocations: &std::collections::BTreeSet<&str>,
    allow_cstring_v2: bool,
    where_: &str,
) -> Result<(), String> {
    let object = value.as_object().ok_or_else(|| format!("{where_} must be an object"))?;
    let allocation = object.get("allocation").and_then(Value::as_str)
        .ok_or_else(|| format!("{where_} is missing string allocation"))?;
    if !known_allocations.contains(allocation) {
        return Err(format!("{where_} references unknown allocation '{allocation}'"));
    }
    let kind = object.get("kind").and_then(Value::as_str)
        .ok_or_else(|| format!("{where_} is missing string kind"))?;
    let certainty = object.get("certainty").and_then(Value::as_str)
        .ok_or_else(|| format!("{where_} is missing string certainty"))?;
    if certainty != "may_abstract" {
        return Err(format!("{where_}.certainty must be 'may_abstract', got '{certainty}'"));
    }
    let effect = object.get("obligation_effect").and_then(Value::as_str)
        .ok_or_else(|| format!("{where_} is missing string obligation_effect"))?;
    let basis = object.get("basis").and_then(Value::as_str)
        .ok_or_else(|| format!("{where_} is missing string basis"))?;
    let (expected_effect, expected_basis, requires_callee) = match kind {
        "box_into_raw" => ("preserve_manual_obligation", "rustc_box_into_raw_v1", true),
        "box_from_raw" => ("restore_raii_obligation", "rustc_box_from_raw_v1", true),
        "box_leak" => ("preserve_persistent_obligation", "rustc_box_leak_v1", true),
        "cstring_into_raw" if allow_cstring_v2 => (
            "preserve_manual_obligation",
            "rustc_cstring_into_raw_v1",
            true,
        ),
        "cstring_from_raw" if allow_cstring_v2 => (
            "restore_raii_obligation",
            "rustc_cstring_from_raw_v1",
            true,
        ),
        "cstring_into_raw" | "cstring_from_raw" => {
            return Err(format!(
                "{where_} kind '{kind}' requires artifact capability allocation_disposition_v2"
            ));
        }
        "mem_forget_owned_box" => ("preserve_unreclaimed_obligation", "rustc_mem_forget_owned_box_v1", true),
        "raw_pointer_drop_noop" => ("no_pointee_lifecycle_effect", "rustc_mem_drop_raw_pointer_v1", true),
        "return_escape" => ("may_escape_to_caller", "rust_return_identity_v1", false),
        "may_deallocate" => ("may_discharge", "allocation_drop_label_v1", false),
        other => return Err(format!("{where_} has unsupported allocation_disposition_v1 kind '{other}'")),
    };
    if effect != expected_effect || basis != expected_basis {
        return Err(format!(
            "{where_} invalid disposition tuple kind={kind} effect={effect} basis={basis}"
        ));
    }
    let callee = object.get("callee_def_path").and_then(Value::as_str);
    if requires_callee && callee.map_or(true, str::is_empty) {
        return Err(format!("{where_} kind '{kind}' requires non-empty callee_def_path"));
    }
    if !requires_callee && callee.is_some() {
        return Err(format!("{where_} kind '{kind}' must not carry call provenance"));
    }
    Ok(())
}

fn validate_boundary_requirements(root: &Value) -> Result<(), String> {
    let Some(schema_version) = root.get("schema_version").and_then(Value::as_u64) else {
        return Err("annotated ICFG is missing integer schema_version".into());
    };
    if schema_version != 2 {
        return Ok(());
    }

    let allocations = root
        .get("allocations")
        .and_then(Value::as_array)
        .ok_or_else(|| "schema-v2 annotated ICFG requires an allocations array".to_string())?;

    let capabilities = root
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|caps| {
            caps.iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let has_allocation_contracts = capabilities.contains("allocation_contracts_v1");
    let has_allocation_contracts_v2 = capabilities.contains("allocation_contracts_v2");
    let has_allocation_contracts_v3 = capabilities.contains("allocation_contracts_v3");
    let has_allocation_state = capabilities.contains("allocation_state_v1");
    let has_allocation_disposition = capabilities.contains("allocation_disposition_v1");
    let has_allocation_disposition_v2 = capabilities.contains("allocation_disposition_v2");
    let has_mir_semantic_labels = capabilities.contains("mir_semantic_labels_v1");
    let has_mir_semantics_v2 = capabilities.contains("mir_semantics_v2");
    let has_panic_unwind_lifecycle_v1 = capabilities.contains("panic_unwind_lifecycle_v1");
    let has_panic_lifecycle_state_v1 = capabilities.contains("panic_lifecycle_state_v1");
    let has_panic_lifecycle_state_v2 = capabilities.contains("panic_lifecycle_state_v2");

    if has_allocation_contracts_v2 && !has_allocation_contracts {
        return Err("allocation_contracts_v2 refines allocation_contracts_v1; the artifact must declare both capabilities".into());
    }
    if has_allocation_contracts_v3 && (!has_allocation_contracts || !has_allocation_contracts_v2) {
        return Err("allocation_contracts_v3 refines allocation_contracts_v2; the artifact must declare allocation_contracts_v1 + allocation_contracts_v2 + allocation_contracts_v3".into());
    }
    if has_mir_semantics_v2 && !has_mir_semantic_labels {
        return Err("mir_semantics_v2 requires mir_semantic_labels_v1 so the active transfer profile remains auditable".into());
    }
    if has_panic_unwind_lifecycle_v1 && (schema_version != 2 || !has_mir_semantics_v2) {
        return Err("panic_unwind_lifecycle_v1 requires schema v2 and mir_semantics_v2".into());
    }
    if has_panic_lifecycle_state_v1 && !has_panic_unwind_lifecycle_v1 {
        return Err("panic_lifecycle_state_v1 requires panic_unwind_lifecycle_v1".into());
    }
    if has_panic_lifecycle_state_v2 && !has_panic_lifecycle_state_v1 {
        return Err("panic_lifecycle_state_v2 refines panic_lifecycle_state_v1; the artifact must declare both capabilities".into());
    }

    if has_allocation_disposition && schema_version != 2 {
        return Err("allocation_disposition_v1 requires annotated ICFG schema v2".into());
    }
    if has_allocation_disposition_v2 && schema_version != 2 {
        return Err("allocation_disposition_v2 requires annotated ICFG schema v2".into());
    }
    if has_allocation_disposition_v2 && !has_allocation_disposition {
        return Err(
            "allocation_disposition_v2 refines allocation_disposition_v1; the artifact must declare both capabilities"
                .into(),
        );
    }

    if has_allocation_contracts {
        for (index, allocation) in allocations.iter().enumerate() {
            let contract = allocation.get("allocator_contract").ok_or_else(|| {
                format!("artifact declares allocation_contracts_v1 but allocations[{index}] is missing allocator_contract")
            })?;
            validate_contract_object(contract, &format!("allocations[{index}].allocator_contract"))?;
        }
    }

    let nodes = root
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "schema-v2 annotated ICFG requires a nodes array".to_string())?;
    for (index, node) in nodes.iter().enumerate() {
        let object = node
            .as_object()
            .ok_or_else(|| format!("schema-v2 nodes[{index}] must be an object"))?;
        for required in ["allocation_labels", "identity", "event_identity"] {
            if !object.contains_key(required) {
                return Err(format!(
                    "schema-v2 nodes[{index}] is missing required field '{required}'; validate the allocation-identity boundary before model checking"
                ));
            }
        }
        if has_mir_semantic_labels {
            let structural = object.get("semantic_labels").ok_or_else(|| format!(
                "artifact declares mir_semantic_labels_v1 but schema-v2 nodes[{index}] is missing semantic_labels"
            ))?;
            let labels = structural.as_array().ok_or_else(|| format!(
                "schema-v2 nodes[{index}].semantic_labels must be an array"
            ))?;
            let mut seen = std::collections::BTreeSet::new();
            for (label_index, label) in labels.iter().enumerate() {
                let label = label.as_str().ok_or_else(|| format!(
                    "schema-v2 nodes[{index}].semantic_labels[{label_index}] must be a string"
                ))?;
                let Some((kind, name)) = label.split_once(':') else {
                    return Err(format!("invalid structural MIR label '{label}'"));
                };
                if !matches!(kind, "stmt" | "rvalue" | "term")
                    || name.is_empty()
                    || !name.chars().all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
                {
                    return Err(format!("invalid structural MIR label '{label}'"));
                }
                if !seen.insert(label) {
                    return Err(format!("duplicate structural MIR label '{label}' in nodes[{index}]"));
                }
            }
        } else if object.get("semantic_labels").is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty())) {
            return Err(format!(
                "schema-v2 nodes[{index}] contains structural MIR labels without capability mir_semantic_labels_v1"
            ));
        }
        if has_allocation_state && !object.contains_key("allocation_post") {
            return Err(format!(
                "artifact declares allocation_state_v1 but schema-v2 nodes[{index}] is missing allocation_post"
            ));
        }
        if has_allocation_disposition && !object.contains_key("allocation_disposition") {
            return Err(format!(
                "artifact declares allocation_disposition_v1 but schema-v2 nodes[{index}] is missing allocation_disposition"
            ));
        }
        if has_panic_lifecycle_state_v1 && !object.contains_key("panic_lifecycle") {
            return Err(format!(
                "artifact declares panic_lifecycle_state_v1 but schema-v2 nodes[{index}] is missing panic_lifecycle"
            ));
        }
        if let Some(records) = object.get("panic_lifecycle") {
            let records = records.as_array().ok_or_else(|| format!(
                "schema-v2 nodes[{index}].panic_lifecycle must be an array"
            ))?;
            let known_allocations: std::collections::BTreeSet<_> = allocations
                .iter()
                .filter_map(|a| a.get("id").and_then(Value::as_str))
                .collect();
            let mut seen = std::collections::BTreeSet::new();
            for (record_index, record) in records.iter().enumerate() {
                let where_ = format!("nodes[{index}].panic_lifecycle[{record_index}]");
                let allocation = record.get("allocation").and_then(Value::as_str)
                    .ok_or_else(|| format!("{where_} is missing string allocation"))?;
                if !known_allocations.contains(allocation) {
                    return Err(format!("{where_} references unknown allocation '{allocation}'"));
                }
                if !seen.insert(allocation) {
                    return Err(format!("nodes[{index}].panic_lifecycle contains duplicate allocation '{allocation}'"));
                }
                let certainty = record.get("certainty").and_then(Value::as_str)
                    .ok_or_else(|| format!("{where_} is missing string certainty"))?;
                if certainty != "may_abstract" {
                    return Err(format!("{where_}.certainty must be 'may_abstract', got '{certainty}'"));
                }
                for field in ["may_own", "may_partial_drop", "may_stale_owner", "may_committed", "may_complete"] {
                    if !record.get(field).is_some_and(Value::is_boolean) {
                        return Err(format!("{where_}.{field} must be a boolean"));
                    }
                }
            }
        }
        if has_panic_lifecycle_state_v2 {
            let coverage = object.get("panic_lifecycle_coverage")
                .and_then(Value::as_str)
                .ok_or_else(|| format!(
                    "artifact declares panic_lifecycle_state_v2 but schema-v2 nodes[{index}] is missing string panic_lifecycle_coverage"
                ))?;
            if !matches!(coverage, "complete" | "unresolved") {
                return Err(format!(
                    "schema-v2 nodes[{index}].panic_lifecycle_coverage must be 'complete' or 'unresolved', got '{coverage}'"
                ));
            }
        } else if object.contains_key("panic_lifecycle_coverage") {
            return Err(format!(
                "schema-v2 nodes[{index}] contains panic_lifecycle_coverage without capability panic_lifecycle_state_v2"
            ));
        }
        if let Some(records) = object.get("allocation_disposition") {
            let records = records.as_array().ok_or_else(|| format!(
                "schema-v2 nodes[{index}].allocation_disposition must be an array"
            ))?;
            let known_allocations: std::collections::BTreeSet<_> = allocations
                .iter()
                .filter_map(|a| a.get("id").and_then(Value::as_str))
                .collect();
            for (record_index, record) in records.iter().enumerate() {
                validate_disposition_object(
                    record,
                    &known_allocations,
                    has_allocation_disposition_v2,
                    &format!("nodes[{index}].allocation_disposition[{record_index}]"),
                )?;
            }
        }
        if let Some(allocation_post) = object.get("allocation_post") {
            let post_object = allocation_post
                .as_object()
                .ok_or_else(|| format!("schema-v2 nodes[{index}].allocation_post must be an object"))?;
            let cells = post_object
                .get("cells")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("schema-v2 nodes[{index}].allocation_post.cells must be an array"))?;
            let known_allocations: std::collections::BTreeSet<_> = allocations
                .iter()
                .filter_map(|a| a.get("id").and_then(Value::as_str))
                .collect();
            let mut seen = std::collections::BTreeSet::new();
            for (cell_index, cell) in cells.iter().enumerate() {
                let allocation = cell
                    .get("allocation")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!(
                        "schema-v2 nodes[{index}].allocation_post.cells[{cell_index}] is missing string allocation"
                    ))?;
                if !known_allocations.contains(allocation) {
                    return Err(format!(
                        "schema-v2 nodes[{index}].allocation_post.cells[{cell_index}] references unknown allocation '{allocation}'"
                    ));
                }
                if !seen.insert(allocation) {
                    return Err(format!(
                        "schema-v2 nodes[{index}].allocation_post contains duplicate allocation '{allocation}'"
                    ));
                }
                if !cell.get("value").is_some_and(Value::is_string) {
                    return Err(format!(
                        "schema-v2 nodes[{index}].allocation_post.cells[{cell_index}] is missing string value"
                    ));
                }
            }
        }
        if !object.get("identity").is_some_and(Value::is_object) {
            return Err(format!("schema-v2 nodes[{index}].identity must be an object"));
        }
        if !object.get("event_identity").is_some_and(Value::is_object) {
            return Err(format!("schema-v2 nodes[{index}].event_identity must be an object"));
        }
        let labels = object
            .get("allocation_labels")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("schema-v2 nodes[{index}].allocation_labels must be an array"))?;
        for (label_index, label) in labels.iter().enumerate() {
            let certainty = label
                .get("certainty")
                .and_then(Value::as_str)
                .ok_or_else(|| format!(
                    "schema-v2 nodes[{index}].allocation_labels[{label_index}] is missing string certainty"
                ))?;
            if certainty != "may_abstract" {
                return Err(format!(
                    "schema-v2 allocation-event certainty must be 'may_abstract', got '{certainty}'"
                ));
            }
            if has_allocation_contracts && label.get("predicate").and_then(Value::as_str) == Some("drop") {
                let contract = label.get("deallocator_contract").ok_or_else(|| {
                    format!("artifact declares allocation_contracts_v1 but nodes[{index}].allocation_labels[{label_index}] drop is missing deallocator_contract")
                })?;
                let where_ = format!("nodes[{index}].allocation_labels[{label_index}].deallocator_contract");
                validate_contract_object(contract, &where_)?;
                if has_allocation_contracts_v2 {
                    validate_v2_deallocator_contract_object(contract, &where_, has_allocation_contracts_v3)?;
                }
            }
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 2 { usage(); }
    let icfg_path = &args[0];
    let query_path = &args[1];
    let mut env0 = Env::new();
    let mut json = false;
    let mut entry_override: Option<String> = None;
    let mut intra = false;
    let mut explain_json: Option<String> = None;
    let mut explain_unk_verbose = false;
    let mut explain_max_witnesses: usize = 8;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => { json = true; i += 1; }
            "--entry" => {
                let Some(entry) = args.get(i + 1) else { return Err("--entry requires NODE_OR_FUNCTION".into()); };
                entry_override = Some(entry.clone());
                i += 2;
            }
            "--intra" => { intra = true; i += 1; }
            "--explain-json" => {
                let Some(path) = args.get(i + 1) else { return Err("--explain-json requires PATH".into()); };
                explain_json = Some(path.clone());
                i += 2;
            }
            "--explain-unk-verbose" => { explain_unk_verbose = true; i += 1; }
            "--explain-max-witnesses" => {
                let Some(raw) = args.get(i + 1) else { return Err("--explain-max-witnesses requires N".into()); };
                explain_max_witnesses = raw.parse::<usize>()
                    .map_err(|_| "--explain-max-witnesses requires a positive integer".to_string())?;
                if explain_max_witnesses == 0 {
                    return Err("--explain-max-witnesses requires N >= 1".into());
                }
                i += 2;
            }
            "--bind" => {
                let Some(binding) = args.get(i + 1) else { return Err("--bind requires name=PROGRAM_VAR_ID".into()); };
                let Some((logic, program)) = binding.split_once('=') else { return Err("--bind requires name=PROGRAM_VAR_ID".into()); };
                if logic.is_empty() || program.is_empty() { return Err("empty side in --bind name=PROGRAM_VAR_ID".into()); }
                env0.insert(logic.to_string(), Binding::ProgramVar(program.to_string()));
                i += 2;
            }
            "--bind-alloc" => {
                let Some(binding) = args.get(i + 1) else { return Err("--bind-alloc requires name=ABSTRACT_ALLOC_ID".into()); };
                let Some((logic, allocation)) = binding.split_once('=') else { return Err("--bind-alloc requires name=ABSTRACT_ALLOC_ID".into()); };
                if logic.is_empty() || allocation.is_empty() { return Err("empty side in --bind-alloc name=ABSTRACT_ALLOC_ID".into()); }
                env0.insert(logic.to_string(), Binding::Allocation(allocation.to_string()));
                i += 2;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    let raw_icfg = fs::read_to_string(icfg_path).map_err(|e| format!("cannot read annotated ICFG '{icfg_path}': {e}"))?;
    let raw_value: Value = serde_json::from_str(&raw_icfg)
        .map_err(|e| format!("invalid annotated ICFG JSON: {e}"))?;
    validate_boundary_requirements(&raw_value)?;
    let panic_lifecycle = panic_lifecycle_overlay_from_json(&raw_value)?;
    let annotated: AnnotatedIcfg = serde_json::from_value(raw_value)
        .map_err(|e| format!("invalid annotated ICFG JSON: {e}"))?;
    let base_k = Kripke::from_annotated_icfg_with_panic_lifecycle(annotated, panic_lifecycle)?;
    let requested_entry = entry_override.as_deref().unwrap_or(&base_k.entry);
    let k = base_k.project_from_entry(requested_entry, intra)?;

    let raw_query = fs::read_to_string(query_path).map_err(|e| format!("cannot read CQPL query '{query_path}': {e}"))?;
    let query = parse_query_document(&raw_query)?;
    let checker = ModelChecker::new(&k);
    let result = checker.evaluate_document(&query, &env0)?;
    let assessment = checker.assess_document(&query, result);

    let need_explanation = explain_json.is_some() || (explain_unk_verbose && result.as_str() == "unk");
    let explanation_report = if need_explanation {
        let report = checker.explain_document(&query, &env0, explain_max_witnesses)?;
        if report.result != result.as_str() {
            return Err(format!(
                "v6R explainability invariant violated: semantic result {} != explanation result {}",
                result.as_str(),
                report.result,
            ));
        }
        if report.assessment != assessment {
            return Err("R2 explainability invariant violated: query assessment differs between direct result and explanation".into());
        }
        Some(report)
    } else {
        None
    };

    if let (Some(path), Some(report)) = (explain_json.as_deref(), explanation_report.as_ref()) {
        let text = serde_json::to_string_pretty(report)
            .map_err(|e| format!("cannot serialize explanation JSON: {e}"))?;
        fs::write(path, format!("{text}\n"))
            .map_err(|e| format!("cannot write explanation JSON '{path}': {e}"))?;
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&JsonOutput {
            result: result.as_str(),
            assessment: &assessment,
            entry: &k.entry,
            scope: if intra { "intra" } else { "reachable" },
            query_file: query_path,
        }).unwrap());
    } else {
        println!("CQPL entry: {}", k.entry);
        println!("CQPL scope: {}", if intra { "intra" } else { "reachable" });
        println!("CQPL result: {}", result.as_str());
        println!("CQPL subresult: {}", assessment.subresult.as_str());
        println!("CQPL strength: {}", assessment.strength.as_str());
        match result.as_str() {
            "ff" => println!("Interpretation: the current annotated abstraction refutes the queried pattern within the modeled predicates."),
            "unk" => println!("Interpretation: the current abstraction cannot refute or establish the queried pattern."),
            "tt" => println!("Interpretation: the formula is established in the annotated abstract Kripke model; this is not by itself a proof of a concrete execution."),
            _ => unreachable!(),
        }
    }

    if explain_unk_verbose && result.as_str() == "unk" {
        let report = explanation_report
            .as_ref()
            .ok_or_else(|| "internal error: verbose UNKNOWN explanation report was not built".to_string())?;
        let query_name = Path::new(query_path)
            .file_stem()
            .and_then(|x| x.to_str())
            .unwrap_or(query_path);
        eprint!("{}", report.render_unknown_verbose(query_name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_v2_node() -> Value {
        json!({
            "schema_version": 2,
            "entry": "b0",
            "variables": [{"id": "rust::main::_1", "language": "rust"}],
            "allocations": [],
            "nodes": [{
                "id": "b0",
                "successors": [],
                "labels": [],
                "allocation_labels": [],
                "identity": {},
                "event_identity": {},
                "pre": {"cells": []},
                "post": {"cells": []}
            }]
        })
    }

    #[test]
    fn schema_v1_cli_guard_preserves_legacy_boundary() {
        let mut value = minimal_v2_node();
        value["schema_version"] = json!(1);
        value.as_object_mut().unwrap().remove("allocations");
        for key in ["allocation_labels", "identity", "event_identity"] {
            value["nodes"][0].as_object_mut().unwrap().remove(key);
        }
        assert!(validate_boundary_requirements(&value).is_ok());
    }

    #[test]
    fn schema_v2_cli_guard_requires_allocations_domain() {
        let mut value = minimal_v2_node();
        value.as_object_mut().unwrap().remove("allocations");
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("allocations"));
    }

    #[test]
    fn schema_v2_cli_guard_requires_event_identity() {
        let mut value = minimal_v2_node();
        value["nodes"][0]
            .as_object_mut()
            .unwrap()
            .remove("event_identity");
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("event_identity"));
    }

    #[test]
    fn schema_v2_cli_guard_rejects_non_may_allocation_certainty() {
        let mut value = minimal_v2_node();
        value["nodes"][0]["allocation_labels"] = json!([{
            "predicate": "drop",
            "allocation": "A",
            "certainty": "exact_abstract"
        }]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("may_abstract"));
    }
    #[test]
    fn capability_cli_guard_requires_allocator_contract_metadata() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!(["allocation_contracts_v1"]);
        value["allocations"] = json!([{
            "id":"A", "display":"A", "site":{"kind":"synthetic","scope":"t","label":"A"}, "context":[]
        }]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("allocator_contract"));
        value["allocations"][0]["allocator_contract"] = json!({
            "family":"rust_global", "operation":"box_allocation", "language":"rust"
        });
        assert!(validate_boundary_requirements(&value).is_ok());
    }

    #[test]
    fn capability_cli_guard_requires_drop_deallocator_contract() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!(["allocation_contracts_v1"]);
        value["allocations"] = json!([{
            "id":"A", "display":"A", "site":{"kind":"synthetic","scope":"t","label":"A"}, "context":[],
            "allocator_contract":{"family":"rust_global","operation":"box_allocation","language":"rust"}
        }]);
        value["nodes"][0]["allocation_labels"] = json!([{
            "predicate":"drop", "allocation":"A", "certainty":"may_abstract"
        }]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("deallocator_contract"));
    }


    #[test]
    fn allocation_contracts_v2_cli_guard_requires_v1_and_closed_basis() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!(["allocation_contracts_v2"]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("must declare both"), "unexpected error: {err}");

        value["capabilities"] = json!(["allocation_contracts_v1", "allocation_contracts_v2"]);
        value["allocations"] = json!([{
            "id":"A", "display":"A", "site":{"kind":"synthetic","scope":"t","label":"A"}, "context":[],
            "allocator_contract":{"family":"rust_global","operation":"box_allocation","language":"rust"}
        }]);
        value["nodes"][0]["allocation_labels"] = json!([{
            "predicate":"drop", "allocation":"A", "certainty":"may_abstract",
            "deallocator_contract":{"family":"rust_global","operation":"drop","language":"rust"}
        }]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("basis"), "unexpected error: {err}");

        value["nodes"][0]["allocation_labels"][0]["deallocator_contract"] = json!({
            "family":"rust_global", "operation":"drop", "language":"rust",
            "basis":"rust_box_global_drop",
            "owner_def_path":"opaque::owner",
            "allocator_def_path":"opaque::allocator"
        });
        assert!(validate_boundary_requirements(&value).is_ok());

        value["nodes"][0]["allocation_labels"][0]["deallocator_contract"]["family"] = json!("c_malloc");
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("requires family=rust_global"), "unexpected error: {err}");
    }

    #[test]
    fn bcontract_drop1_v3_cli_guard_gates_new_basis() {
        let mut value = minimal_v2_node();
        value["allocations"] = json!([{
            "id":"A", "display":"A", "site":{"kind":"synthetic","scope":"t","label":"A"}, "context":[],
            "allocator_contract":{"family":"rust_global","operation":"box_allocation","language":"rust"}
        }]);
        value["nodes"][0]["allocation_labels"] = json!([{
            "predicate":"drop", "allocation":"A", "certainty":"may_abstract",
            "deallocator_contract":{
                "family":"rust_global",
                "operation":"drop",
                "language":"rust",
                "basis":"rust_mem_drop_owned_box_global_v1",
                "owner_def_path":"alloc::boxed::Box",
                "allocator_def_path":"alloc::alloc::Global",
                "callee_def_path":"core::mem::drop"
            }
        }]);

        value["capabilities"] = json!(["allocation_contracts_v1", "allocation_contracts_v2"]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("requires artifact capability allocation_contracts_v3"), "unexpected error: {err}");

        value["capabilities"] = json!([
            "allocation_contracts_v1",
            "allocation_contracts_v2",
            "allocation_contracts_v3"
        ]);
        assert!(validate_boundary_requirements(&value).is_ok());

        value["nodes"][0]["allocation_labels"][0]["deallocator_contract"]["callee_def_path"] = Value::Null;
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("requires non-empty owner_def_path, allocator_def_path, and callee_def_path"), "unexpected error: {err}");
    }

    #[test]
    fn b1_1_disposition_v2_cli_guard_gates_cstring_kinds() {
        let mut value = minimal_v2_node();
        value["allocations"] = json!([{
            "id":"A", "display":"A", "site":{"kind":"synthetic","scope":"t","label":"A"}, "context":[]
        }]);
        value["nodes"][0]["allocation_disposition"] = json!([{
            "allocation":"A",
            "kind":"cstring_into_raw",
            "certainty":"may_abstract",
            "obligation_effect":"preserve_manual_obligation",
            "basis":"rustc_cstring_into_raw_v1",
            "callee_def_path":"alloc::ffi::c_str::CString::into_raw"
        }]);

        value["capabilities"] = json!(["allocation_disposition_v1"]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("requires artifact capability allocation_disposition_v2"), "unexpected error: {err}");

        value["capabilities"] = json!(["allocation_disposition_v2"]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("must declare both capabilities"), "unexpected error: {err}");

        value["capabilities"] = json!(["allocation_disposition_v1", "allocation_disposition_v2"]);
        assert!(validate_boundary_requirements(&value).is_ok());
    }

    #[test]
    fn b1_1_r1_cli_guard_accepts_cstring_drop_and_rejects_wrong_family() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!(["allocation_contracts_v1", "allocation_contracts_v2"]);
        value["allocations"] = json!([{
            "id":"A", "display":"A", "site":{"kind":"synthetic","scope":"t","label":"A"}, "context":[],
            "allocator_contract":{"family":"rust_global","operation":"cstring_allocation","language":"rust"}
        }]);
        value["nodes"][0]["allocation_labels"] = json!([{
            "predicate":"drop", "allocation":"A", "certainty":"may_abstract",
            "deallocator_contract":{
                "family":"rust_global",
                "operation":"drop",
                "language":"rust",
                "basis":"rust_cstring_global_drop",
                "owner_def_path":"alloc::ffi::c_str::CString",
                "allocator_def_path":"alloc::alloc::Global"
            }
        }]);
        assert!(validate_boundary_requirements(&value).is_ok());

        value["nodes"][0]["allocation_labels"][0]["deallocator_contract"]["family"] = json!("c_malloc");
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("requires family=rust_global"), "unexpected error: {err}");
    }

    #[test]
    fn panic_unwind_cli_guard_requires_mir_v2() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!(["panic_unwind_lifecycle_v1"]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("panic_unwind_lifecycle_v1"), "unexpected error: {err}");

        value["capabilities"] = json!([
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1"
        ]);
        value["nodes"][0]["semantic_labels"] = json!([]);
        assert!(validate_boundary_requirements(&value).is_ok());
    }

    #[test]
    fn panic_lifecycle_boundary_requires_may_abstract_boolean_records() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!([
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1",
            "panic_lifecycle_state_v1"
        ]);
        value["allocations"] = json!([{
            "id": "A", "display": "A", "site": null, "context": []
        }]);
        value["nodes"][0]["semantic_labels"] = json!([]);
        value["nodes"][0]["panic_lifecycle"] = json!([{
            "allocation": "A",
            "certainty": "may_abstract",
            "may_own": true,
            "may_partial_drop": true,
            "may_stale_owner": true,
            "may_committed": false,
            "may_complete": false
        }]);
        assert!(validate_boundary_requirements(&value).is_ok());

        value["nodes"][0]["panic_lifecycle"][0]["certainty"] = json!("exact_abstract");
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("may_abstract"), "unexpected error: {err}");
    }

    #[test]
    fn panic_lifecycle_boundary_requires_field_on_every_node_when_capability_is_declared() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!([
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1",
            "panic_lifecycle_state_v1"
        ]);
        value["nodes"][0]["semantic_labels"] = json!([]);
        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("missing panic_lifecycle"), "unexpected error: {err}");
    }

    #[test]
    fn panic_lifecycle_v2_boundary_requires_explicit_coverage() {
        let mut value = minimal_v2_node();
        value["capabilities"] = json!([
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1",
            "panic_lifecycle_state_v1",
            "panic_lifecycle_state_v2"
        ]);
        value["nodes"][0]["semantic_labels"] = json!([]);
        value["nodes"][0]["panic_lifecycle"] = json!([]);

        let err = validate_boundary_requirements(&value).unwrap_err();
        assert!(err.contains("panic_lifecycle_coverage"), "unexpected error: {err}");

        value["nodes"][0]["panic_lifecycle_coverage"] = json!("complete");
        assert!(validate_boundary_requirements(&value).is_ok());

        value["nodes"][0]["panic_lifecycle_coverage"] = json!("unresolved");
        assert!(validate_boundary_requirements(&value).is_ok());
    }


}
