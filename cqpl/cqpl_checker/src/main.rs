use cqpl_checker::{parse_query, AnnotatedIcfg, Env, Kripke, ModelChecker};
use serde::Serialize;
use std::{env, fs, process};

#[derive(Serialize)]
struct JsonOutput<'a> {
    result: &'a str,
    entry: &'a str,
    query_file: &'a str,
}

fn usage() -> ! {
    eprintln!("Usage: cqpl_checker <annotated-icfg.json> <query.cqpl> [--bind x=PROGRAM_VAR_ID]... [--json]");
    process::exit(2);
}

fn main() {
    if let Err(e) = run() {
        eprintln!("cqpl_checker: {e}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 2 { usage(); }
    let icfg_path = &args[0];
    let query_path = &args[1];
    let mut env0 = Env::new();
    let mut json = false;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => { json = true; i += 1; }
            "--bind" => {
                let Some(binding) = args.get(i + 1) else { return Err("--bind requires name=PROGRAM_VAR_ID".into()); };
                let Some((logic, program)) = binding.split_once('=') else { return Err("--bind requires name=PROGRAM_VAR_ID".into()); };
                if logic.is_empty() || program.is_empty() { return Err("empty side in --bind name=PROGRAM_VAR_ID".into()); }
                env0.insert(logic.to_string(), program.to_string());
                i += 2;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    let raw_icfg = fs::read_to_string(icfg_path).map_err(|e| format!("cannot read annotated ICFG '{icfg_path}': {e}"))?;
    let annotated: AnnotatedIcfg = serde_json::from_str(&raw_icfg).map_err(|e| format!("invalid annotated ICFG JSON: {e}"))?;
    let k = Kripke::from_annotated_icfg(annotated)?;

    let raw_query = fs::read_to_string(query_path).map_err(|e| format!("cannot read CQPL query '{query_path}': {e}"))?;
    let query = parse_query(&raw_query)?;
    let result = ModelChecker::new(&k).evaluate(&query, &env0)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&JsonOutput { result: result.as_str(), entry: &k.entry, query_file: query_path }).unwrap());
    } else {
        println!("CQPL result: {}", result.as_str());
        match result.as_str() {
            "ff" => println!("Interpretation: the annotated abstraction refutes the queried pattern."),
            "unk" => println!("Interpretation: potential match; the sound may abstraction does not refute the queried pattern."),
            "tt" => println!("Interpretation: the formula is established by exact/three-valued composition at the entry state."),
            _ => unreachable!(),
        }
    }
    Ok(())
}
