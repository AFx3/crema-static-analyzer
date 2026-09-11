#!/usr/bin/env python3
import argparse, collections, json, re
from pathlib import Path

SCALAR_CONST = re.compile(r"^const (?:true|false|\(\)|-?[0-9]+_(?:i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize)|-?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?_(?:f32|f64)|'(?:\\.|[^\\'])+')$")
DIRECT_USE = re.compile(r"^(?:copy|move) _\d+$")
DIRECT_DEREF_USE = re.compile(r"^(?:copy|move) \(\*_\d+\)$")
CMP = re.compile(r"^(?:Eq|Lt|Le|Ne|Ge|Gt)\(")
OFFSET = re.compile(r"^Offset\(")
ARITH = re.compile(r"^(?:Add|Sub|Mul|Div|Rem|BitXor|BitAnd|BitOr|Shl|Shr)\(")
CHECKED = re.compile(r"^(?:AddWithOverflow|SubWithOverflow|MulWithOverflow|AddUnchecked|SubUnchecked|MulUnchecked|ShlUnchecked|ShrUnchecked)\(")
UNARY = re.compile(r"^(?:Neg|Not)\(")
SCALAR_NULLARY = re.compile(r"^(?:SizeOf|AlignOf|Len|Discriminant)\(")
CAST = re.compile(r".+ as (.+?) \([^()]+\)$")
RAW_PTR_TY = re.compile(r"^\*(?:mut|const)\b")

MEMORY_CALL_RULES = [
    ("mem_forget", lambda s: "mem::forget" in s),
    ("mem_drop", lambda s: "mem::drop" in s),
    ("box_new", lambda s: "Box::<" in s and ">::new" in s),
    ("box_into_raw", lambda s: "Box::<" in s and ">::into_raw" in s),
    ("box_from_raw", lambda s: "Box::<" in s and ">::from_raw" in s),
    ("box_leak", lambda s: "Box::<" in s and ">::leak" in s),
    ("cstring_into_raw", lambda s: "CString::into_raw" in s),
    ("cstring_from_raw", lambda s: "CString::from_raw" in s),
    ("cstr_from_ptr", lambda s: "CStr::from_ptr" in s or "c_str::<impl" in s and "from_ptr" in s),
    ("borrowed_raw_pointer_view", lambda s: ("::as_ptr" in s or "::as_mut_ptr" in s) and any(t in s for t in ["CString","CStr","Vec::<","String","NonNull::<","str>::as_ptr","slice::<impl ["])),
    ("vec_from_raw_parts", lambda s: "Vec::<" in s and "from_raw_parts" in s),
    ("string_from_raw_parts", lambda s: "String::from_raw_parts" in s),
    ("alloc_zeroed", lambda s: "alloc_zeroed" in s),
    ("dealloc", lambda s: "::dealloc" in s),
    ("realloc", lambda s: "::realloc" in s),
    ("alloc", lambda s: ("::alloc" in s and "alloc_zeroed" not in s and "dealloc" not in s and "realloc" not in s and "handle_alloc_error" not in s)),
    ("ptr_read", lambda s: "ptr::read" in s),
    ("ptr_write", lambda s: "ptr::write" in s),
    ("ptr_drop_in_place", lambda s: "ptr::drop_in_place" in s),
    ("rc_raw", lambda s: "Rc::<" in s and ("into_raw" in s or "from_raw" in s)),
    ("arc_raw", lambda s: "Arc::<" in s and ("into_raw" in s or "from_raw" in s)),
    ("manually_drop", lambda s: "ManuallyDrop" in s),
]

PHASE4_HANDLED = {
    "scalar_const", "direct_use", "ref", "raw_address", "scalar_cast",
    "pointer_cast", "comparison", "pointer_offset", "arithmetic_binary",
    "unary", "scalar_nullary", "closure_aggregate", "copy_for_deref",
    "direct_deref_use",
}

def classify_rvalue(rv: str) -> str:
    s = (rv or "").strip()
    if not s:
        return "empty"
    if SCALAR_CONST.match(s):
        return "scalar_const"
    if DIRECT_USE.match(s):
        return "direct_use"
    if DIRECT_DEREF_USE.match(s):
        return "direct_deref_use"
    if s.startswith("&raw const ") or s.startswith("&raw mut "):
        return "raw_address"
    if s.startswith("&"):
        return "ref"
    if s.startswith("{closure@"):
        return "closure_aggregate"
    if s.startswith("deref_copy "):
        return "copy_for_deref"
    if CMP.match(s):
        return "comparison"
    if OFFSET.match(s):
        return "pointer_offset"
    if CHECKED.match(s):
        return "checked_binary_unhandled"
    if ARITH.match(s):
        return "arithmetic_binary"
    if UNARY.match(s):
        return "unary"
    if SCALAR_NULLARY.match(s):
        return "scalar_nullary"
    m = CAST.match(s)
    if m:
        ty = m.group(1).strip()
        scalar = {"bool","char","i8","i16","i32","i64","i128","isize","u8","u16","u32","u64","u128","usize","f32","f64","()"}
        if ty in scalar:
            return "scalar_cast"
        if RAW_PTR_TY.match(ty):
            return "pointer_cast"
        return "other_cast_unhandled"
    if s.startswith("[") and ";" in s:
        return "repeat_unhandled"
    if s.startswith("(") or s.startswith("[") or s.startswith("Adt(") or s.startswith("Closure(") or s.startswith("Coroutine("):
        return "aggregate_unhandled"
    if "ThreadLocal" in s:
        return "thread_local_unhandled"
    return "unknown_unhandled"

def call_rule(name: str):
    for label, pred in MEMORY_CALL_RULES:
        try:
            if pred(name):
                return label
        except Exception:
            pass
    return None

def census_one(path: Path, target: str):
    d = json.loads(path.read_text())
    rv_counts = collections.Counter()
    rv_examples = collections.defaultdict(list)
    term_counts = collections.Counter()
    call_counts = collections.Counter()
    mem_calls = collections.Counter()
    mem_examples = collections.defaultdict(list)

    for node_id, node in d.get("ordered_nodes", []):
        if not isinstance(node, dict) or node.get("node_type") != "Mir":
            continue
        nd = node.get("node_data") or {}
        for st in nd.get("statements") or []:
            if st.get("kind") == "Assign":
                rv = st.get("rvalue") or ""
                k = classify_rvalue(rv)
                rv_counts[k] += 1
                if len(rv_examples[k]) < 8 and rv not in rv_examples[k]:
                    rv_examples[k].append(rv)
        term = nd.get("terminator") or {}
        kind = term.get("kind") or "UNKNOWN"
        term_counts[kind] += 1
        if kind == "Call":
            fn = term.get("function_called") or "<unknown-call>"
            call_counts[fn] += 1
            rule = call_rule(fn)
            if rule:
                mem_calls[rule] += 1
                if len(mem_examples[rule]) < 8 and fn not in mem_examples[rule]:
                    mem_examples[rule].append(fn)

    unhandled = {k:v for k,v in rv_counts.items() if k not in PHASE4_HANDLED}
    return {
        "schema_version": 1,
        "target": target,
        "source_icfg": str(path),
        "rvalue_total": sum(rv_counts.values()),
        "rvalue_kind_counts": dict(sorted(rv_counts.items())),
        "rvalue_unhandled_counts": dict(sorted(unhandled.items())),
        "rvalue_examples": dict(sorted(rv_examples.items())),
        "terminator_kind_counts": dict(sorted(term_counts.items())),
        "call_total": sum(call_counts.values()),
        "call_counts": dict(sorted(call_counts.items())),
        "memory_api_counts": dict(sorted(mem_calls.items())),
        "memory_api_examples": dict(sorted(mem_examples.items())),
    }

def aggregate(paths, out_path=None):
    items=[]
    rv=collections.Counter(); un=collections.Counter(); term=collections.Counter(); calls=collections.Counter(); mem=collections.Counter()
    examples=collections.defaultdict(list); memex=collections.defaultdict(list)
    for p in paths:
        d=json.loads(Path(p).read_text()); items.append(d.get("target",Path(p).stem))
        rv.update(d.get("rvalue_kind_counts",{})); un.update(d.get("rvalue_unhandled_counts",{})); term.update(d.get("terminator_kind_counts",{})); calls.update(d.get("call_counts",{})); mem.update(d.get("memory_api_counts",{}))
        for k, vals in d.get("rvalue_examples",{}).items():
            for x in vals:
                if x not in examples[k] and len(examples[k])<20: examples[k].append(x)
        for k, vals in d.get("memory_api_examples",{}).items():
            for x in vals:
                if x not in memex[k] and len(memex[k])<20: memex[k].append(x)
    result={
        "schema_version":1,
        "targets":len(items),
        "target_keys":sorted(items),
        "rvalue_kind_counts":dict(sorted(rv.items())),
        "rvalue_unhandled_counts":dict(sorted(un.items())),
        "rvalue_examples":dict(sorted(examples.items())),
        "terminator_kind_counts":dict(sorted(term.items())),
        "memory_api_counts":dict(sorted(mem.items())),
        "memory_api_examples":dict(sorted(memex.items())),
        "top_calls":calls.most_common(100),
    }
    text=json.dumps(result,indent=2,sort_keys=True)+"\n"
    if out_path: Path(out_path).write_text(text)
    else: print(text,end="")


def main():
    ap=argparse.ArgumentParser()
    sub=ap.add_subparsers(dest="cmd",required=True)
    one=sub.add_parser("one"); one.add_argument("icfg"); one.add_argument("target"); one.add_argument("-o","--output")
    agg=sub.add_parser("aggregate"); agg.add_argument("inputs",nargs="+"); agg.add_argument("-o","--output")
    ns=ap.parse_args()
    if ns.cmd=="one":
        res=census_one(Path(ns.icfg),ns.target); text=json.dumps(res,indent=2,sort_keys=True)+"\n"
        if ns.output: Path(ns.output).write_text(text)
        else: print(text,end="")
    else: aggregate(ns.inputs,ns.output)
if __name__=="__main__": main()
