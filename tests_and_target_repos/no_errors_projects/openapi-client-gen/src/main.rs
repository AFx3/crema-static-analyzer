#![warn(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use heck::{ToSnakeCase, ToUpperCamelCase};
use json::JsonValue;
use json::object::Object;

mod rt;

#[cfg(test)]
mod test;


#[derive(Debug, Clone)]
struct Operation {
    pub counter: usize,
    pub name: String,
    pub response_name: String,
    pub function_name: String,
    pub async_function_name: String,
    pub method: String,
    pub path: String,
    pub element: JsonValue,
}


static OPERATION_COUNTER: AtomicUsize = AtomicUsize::new(0usize);
impl Operation {
    pub fn new(name: &str, path: &str, method: &str, element: &JsonValue) -> Operation {

        Operation {
            counter: OPERATION_COUNTER.fetch_add(1, Ordering::SeqCst),
            name: name.to_string(),
            function_name : name.to_snake_case(),
            async_function_name: format!("async_{}", name.to_snake_case()),
            response_name: name.to_upper_camel_case() + "Rsp",
            path: path.to_string(),
            method: method.to_uppercase(),
            element: element.clone(),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    ffi_op_prefix: String,
    ffi_accessor_prefix: String,
    struct_name_prefix: String,
    ffi_prefix: String,
    struct_name_map: HashMap<String, String>,
    poly_map: HashMap<String, HashSet<String>>,
    map_types: Vec<String>,
    operations: Vec<Operation>,
    main_buffer: Vec<u8>,
    path_buffer: Vec<u8>,
    async_path_buffer: Vec<u8>,
    ffi_buffer: Vec<u8>,
}

impl State {
    fn push<T: ToString>(&mut self, data: T) {
        self.main_buffer.write_all(data.to_string().as_bytes()).unwrap();
    }

    fn push_path<T: ToString>(&mut self, data: T) {
        self.path_buffer.write_all(data.to_string().as_bytes()).unwrap();
    }

    fn push_async_path<T: ToString>(&mut self, data: T) {
        self.async_path_buffer.write_all(data.to_string().as_bytes()).unwrap();
    }


    fn push_ffi<T: ToString>(&mut self, data: T) {
        self.ffi_buffer.write_all(data.to_string().as_bytes()).unwrap();
    }

    fn insert_path(&mut self) {
        self.main_buffer.write_all(self.path_buffer.as_slice()).unwrap();
        self.path_buffer = Vec::new();
    }
    fn insert_async_path(&mut self) {
        self.main_buffer.write_all(self.async_path_buffer.as_slice()).unwrap();
        self.async_path_buffer = Vec::new();
    }

    fn insert_ffi(&mut self) {
        self.main_buffer.write_all(self.ffi_buffer.as_slice()).unwrap();
        self.ffi_buffer = Vec::new();
    }
}

#[derive(Clone, Debug)]
enum Schema {
    Invalid,
    String,
    Int64,
    Int32,
    Double,
    Float,
    Boolean,
    Any,
    Ref(String),
    PolymorphicObjectImpl(JsonValue),
    CompositeAllObjectImpl(JsonValue),
    CompositeAnyObjectImpl(JsonValue),
    CompositeOneObjectImpl(JsonValue),
    ObjectImpl(JsonValue),
    RefArray(String),
    ImplArray(JsonValue),
    StringArray,
    Int64Array,
    Int32Array,
    DoubleArray,
    FloatArray,
    BooleanArray,
    AnyArray,
    AnyMap,
    ArrayMap(JsonValue),
    StringMap,
    Int64Map,
    Int32Map,
    DoubleMap,
    FloatMap,
    BooleanMap,
    RefMap(String),
    ImplMap(JsonValue),
    Constant(JsonValue),
}

impl Display for Schema {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Schema::Invalid => "Invalid",
            Schema::String => "String",
            Schema::Int64 => "Int64",
            Schema::Int32 => "Int32",
            Schema::Any => "Any",
            Schema::Ref(_) => "Ref",
            Schema::PolymorphicObjectImpl(_) => "PolymorphicObjectImpl",
            Schema::CompositeAllObjectImpl(_) => "CompositeAllObjectImpl",
            Schema::ObjectImpl(_) => "ObjectImpl",
            Schema::RefArray(_) => "RefArray",
            Schema::ImplArray(_) => "ImplArray",
            Schema::StringArray => "StringArray",
            Schema::Int64Array => "Int64Array",
            Schema::Int32Array => "Int32Array",
            Schema::AnyArray => "AnyArray",
            Schema::AnyMap => "AnyMap",
            Schema::StringMap => "StringMap",
            Schema::Int64Map => "Int64Map",
            Schema::Int32Map => "Int32Map",
            Schema::RefMap(_) => "RefMap",
            Schema::ImplMap(_) => "ImplMap",
            Schema::Double => "Double",
            Schema::Float => "Float",
            Schema::DoubleArray => "DoubleArray",
            Schema::DoubleMap => "DoubleMap",
            Schema::Boolean => "Boolean",
            Schema::BooleanArray => "BooleanArray",
            Schema::BooleanMap => "BooleanMap",
            Schema::ArrayMap(_) => "ArrayMap",
            Schema::FloatArray => "FloatArray",
            Schema::FloatMap => "FloatMap",
            Schema::CompositeAnyObjectImpl(_) => "CompositeAnyObjectImpl",
            Schema::CompositeOneObjectImpl(_) => "CompositeOneObjectImpl",
            Schema::Constant(_) => "Constant",
        };

        f.write_str(name)
    }
}

fn move_schema_implementation<T: ToString>(implementation: &JsonValue, preferred_name: T, movement_root: &mut JsonValue) -> JsonValue {
    let copy = implementation.clone();
    let base_name = preferred_name.to_string();
    let mut idx = 0;
    loop {
        let name = match idx {
            0 => base_name.clone(),
            _=> format!("{}{}", base_name.as_str(), idx)
        };
        idx+=1;
        if !movement_root["components"]["schemas"][name.as_str()].is_null() {
            continue;
        }

        movement_root["components"]["schemas"][name.as_str()] = copy;
        let mut reference_obj = JsonValue::Object(Object::new());
        reference_obj["$ref"] = JsonValue::String(format!("#/components/schemas/{}", name.as_str()));
        return reference_obj;
    }
}

fn classify_schema(schema: &JsonValue) -> Schema {
    if !schema["$ref"].is_null() {
        let refo = schema["$ref"].as_str();
        if refo.is_none() {
            return Schema::Invalid
        }

        let refo = refo.unwrap().to_string();
        if !refo.starts_with("#/components/schemas/") {
            return Schema::Invalid
        }

        let name = refo.as_str()[21..].to_string();
        return Schema::Ref(name);
    }

    if !schema["allOf"].is_null() {
        if schema["allOf"].is_array() {
            return Schema::CompositeAllObjectImpl(schema["allOf"].clone());
        }

        return Schema::Invalid
    }

    if !schema["anyOf"].is_null() {
        if schema["anyOf"].is_array() {
            return Schema::CompositeAnyObjectImpl(schema["anyOf"].clone());
        }

        return Schema::Invalid
    }

    if !schema["oneOf"].is_null() {
        if schema["oneOf"].is_array() {
            return Schema::CompositeOneObjectImpl(schema["oneOf"].clone());
        }

        return Schema::Invalid
    }

    if !schema["const"].is_null() {
        return Schema::Constant(schema["const"].clone());
    }


    match schema["type"].as_str() {
        Some("string") => Schema::String,
        Some("integer") | Some("number") => match schema["format"].as_str() {
            Some("int64") => Schema::Int64,
            Some("int32") => Schema::Int32,
            Some("double") => Schema::Double,
            Some("float") => Schema::Float,
            _=> Schema::Invalid
        }
        Some("boolean") => Schema::Boolean,
        Some("array") => {
            if !schema["items"].is_object() {
                return Schema::Invalid
            }

            match classify_schema(&schema["items"]) {
                Schema::String => Schema::StringArray,
                Schema::Int64 => Schema::Int64Array,
                Schema::Int32 => Schema::Int32Array,
                Schema::Double => Schema::DoubleArray,
                Schema::Float => Schema::FloatArray,
                Schema::Boolean => Schema::BooleanArray,
                Schema::Any => Schema::AnyArray,
                Schema::Ref(name) => Schema::RefArray(name),
                Schema::CompositeAllObjectImpl(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::CompositeAnyObjectImpl(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::CompositeOneObjectImpl(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::ObjectImpl(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::RefArray(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::ImplArray(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::StringArray => Schema::ImplArray(schema["items"].clone()),
                Schema::Int64Array => Schema::ImplArray(schema["items"].clone()),
                Schema::Int32Array => Schema::ImplArray(schema["items"].clone()),
                Schema::BooleanArray => Schema::ImplArray(schema["items"].clone()),
                Schema::AnyArray =>Schema::ImplArray(schema["items"].clone()),
                Schema::AnyMap => Schema::ImplArray(schema["items"].clone()),
                Schema::StringMap => Schema::ImplArray(schema["items"].clone()),
                Schema::Int64Map => Schema::ImplArray(schema["items"].clone()),
                Schema::Int32Map => Schema::ImplArray(schema["items"].clone()),
                Schema::DoubleMap => Schema::ImplArray(schema["items"].clone()),
                Schema::FloatMap => Schema::ImplArray(schema["items"].clone()),
                Schema::BooleanMap => Schema::ImplArray(schema["items"].clone()),
                Schema::RefMap(_) => Schema::ImplArray(schema["items"].clone()),
                Schema::ImplMap(_) => Schema::ImplArray(schema["items"].clone()),
                _=>  Schema::Invalid
            }
        }
        Some("object") => {
            if schema["properties"].is_object() {
                if !schema["discriminator"].is_null() {
                    return Schema::PolymorphicObjectImpl(schema.clone())
                }
                return Schema::ObjectImpl(schema.clone());
            }
            if schema["additionalProperties"].is_object() {
                if !schema["discriminator"].is_null() {
                    return Schema::Invalid
                }
                if schema["additionalProperties"]["$ref"].is_string() {
                    let refo = schema["additionalProperties"]["$ref"].as_str().unwrap().to_string();
                    if !refo.starts_with("#/components/schemas/") {
                        return Schema::Invalid
                    }

                    let name = refo.as_str()[21..].to_string();
                    return Schema::RefMap(name);
                }
                match schema["additionalProperties"]["type"].as_str() {
                    Some("string") => return Schema::StringMap,
                    Some("integer") | Some("number") => match schema["additionalProperties"]["format"].as_str() {
                        Some("int64") => return Schema::Int64Map,
                        Some("int32") => return Schema::Int32Map,
                        Some("double") => return Schema::DoubleMap,
                        Some("float") => return Schema::FloatMap,
                        _=> return Schema::Invalid
                    }
                    Some("boolean") => return Schema::BooleanMap,
                    Some("object") => {
                        if !schema["additionalProperties"]["additionalProperties"].is_null() {
                            return Schema::ImplMap(schema["additionalProperties"].clone())
                        }
                        if !schema["additionalProperties"]["properties"].is_null() {
                            return Schema::ImplMap(schema["additionalProperties"].clone())
                        }

                        return Schema::AnyMap
                    }
                    Some("array") => {
                        return Schema::ArrayMap(schema["additionalProperties"].clone())
                    }
                    _=> return Schema::Invalid
                }
            }

            Schema::Any
        }
        _ => Schema::Invalid
    }
}

fn capitalize_first_letter(s: &str) -> String {
    s[0..1].to_uppercase() + &s[1..]
}

fn collect_map_types(state: &mut State, root: &JsonValue) {
    let mut maps = HashSet::new();

    for (_name, schema) in root["components"]["schemas"].entries() {
        match classify_schema(schema) {
            Schema::RefMap(refer) => {
                maps.insert(refer);
            }
            Schema::ObjectImpl(impl_schema) => {
                for (_field_name, field) in impl_schema["properties"].entries() {
                    match classify_schema(field) {
                        Schema::RefMap(refer) => {
                            maps.insert(refer);
                        }
                        _=> {}
                    }
                }
            }
            _=> {}
        }
    }

    for x in maps.iter() {
        state.map_types.push(x.to_string());
    }
    state.map_types.sort();
}

fn collect_struct_names(state: &mut State, root: &JsonValue) {

    let mut occupied_struct_name = HashSet::new();
    occupied_struct_name.insert("Display".to_string());
    occupied_struct_name.insert("Into".to_string());
    occupied_struct_name.insert("ApiClient".to_string());
    occupied_struct_name.insert("From".to_string());
    occupied_struct_name.insert("Any".to_string());
    occupied_struct_name.insert("HashMap".to_string());
    occupied_struct_name.insert("Debug".to_string());
    occupied_struct_name.insert("Formatter".to_string());
    occupied_struct_name.insert("Write".to_string());
    occupied_struct_name.insert("FromStr".to_string());
    occupied_struct_name.insert("Either".to_string());
    occupied_struct_name.insert("HeaderMap".to_string());
    occupied_struct_name.insert("HeaderName".to_string());
    occupied_struct_name.insert("HeaderValue".to_string());
    occupied_struct_name.insert("Method".to_string());
    occupied_struct_name.insert("StatusCode".to_string());
    occupied_struct_name.insert("JsonValue".to_string());
    occupied_struct_name.insert("Number".to_string());
    occupied_struct_name.insert("JsonObject".to_string());
    occupied_struct_name.insert("LinkedHashMap".to_string());
    occupied_struct_name.insert("Body".to_string());
    occupied_struct_name.insert("Client".to_string());
    occupied_struct_name.insert("Request".to_string());
    occupied_struct_name.insert("RequestBuilder".to_string());
    occupied_struct_name.insert("Response".to_string());
    occupied_struct_name.insert("Url".to_string());
    occupied_struct_name.insert("CStr".to_string());
    occupied_struct_name.insert("Read".to_string());
    occupied_struct_name.insert("Deref".to_string());
    occupied_struct_name.insert("DerefMut".to_string());
    occupied_struct_name.insert("Rc".to_string());
    occupied_struct_name.insert("Arc".to_string());
    occupied_struct_name.insert("Mutex".to_string());
    occupied_struct_name.insert("PoisonError".to_string());
    occupied_struct_name.insert("String".to_string());
    occupied_struct_name.insert("OString".to_string());
    occupied_struct_name.insert("OI64".to_string());
    occupied_struct_name.insert("OI32".to_string());
    occupied_struct_name.insert("OI16".to_string());
    occupied_struct_name.insert("OI8".to_string());
    occupied_struct_name.insert("OU64".to_string());
    occupied_struct_name.insert("OU32".to_string());
    occupied_struct_name.insert("OU16".to_string());
    occupied_struct_name.insert("OU8".to_string());
    occupied_struct_name.insert("OF64".to_string());
    occupied_struct_name.insert("OF32".to_string());
    occupied_struct_name.insert("OBool".to_string());

    occupied_struct_name.insert("OStringArray".to_string());
    occupied_struct_name.insert("OI64Array".to_string());
    occupied_struct_name.insert("OI32Array".to_string());
    occupied_struct_name.insert("OI16Array".to_string());
    occupied_struct_name.insert("OI8Array".to_string());
    occupied_struct_name.insert("OU64Array".to_string());
    occupied_struct_name.insert("OU32Array".to_string());
    occupied_struct_name.insert("OU16Array".to_string());
    occupied_struct_name.insert("OU8Array".to_string());
    occupied_struct_name.insert("OF64Array".to_string());
    occupied_struct_name.insert("OF32Array".to_string());
    occupied_struct_name.insert("OBoolArray".to_string());

    occupied_struct_name.insert("StringArray".to_string());
    occupied_struct_name.insert("I64Array".to_string());
    occupied_struct_name.insert("I32Array".to_string());
    occupied_struct_name.insert("I16Array".to_string());
    occupied_struct_name.insert("I8Array".to_string());
    occupied_struct_name.insert("U64Array".to_string());
    occupied_struct_name.insert("U32Array".to_string());
    occupied_struct_name.insert("U16Array".to_string());
    occupied_struct_name.insert("U8Array".to_string());
    occupied_struct_name.insert("F64Array".to_string());
    occupied_struct_name.insert("F32Array".to_string());
    occupied_struct_name.insert("BoolArray".to_string());

    occupied_struct_name.insert("OStringMap".to_string());
    occupied_struct_name.insert("OI64Map".to_string());
    occupied_struct_name.insert("OI32Map".to_string());
    occupied_struct_name.insert("OI16Map".to_string());
    occupied_struct_name.insert("OI8Map".to_string());
    occupied_struct_name.insert("OU64Map".to_string());
    occupied_struct_name.insert("OU32Map".to_string());
    occupied_struct_name.insert("OU16Map".to_string());
    occupied_struct_name.insert("OU8Map".to_string());
    occupied_struct_name.insert("OF64Map".to_string());
    occupied_struct_name.insert("OF32Map".to_string());
    occupied_struct_name.insert("OBoolMap".to_string());

    occupied_struct_name.insert("StringMap".to_string());
    occupied_struct_name.insert("I64Map".to_string());
    occupied_struct_name.insert("I32Map".to_string());
    occupied_struct_name.insert("I16Map".to_string());
    occupied_struct_name.insert("I8Map".to_string());
    occupied_struct_name.insert("U64Map".to_string());
    occupied_struct_name.insert("U32Map".to_string());
    occupied_struct_name.insert("U16Map".to_string());
    occupied_struct_name.insert("U8Map".to_string());
    occupied_struct_name.insert("F64Map".to_string());
    occupied_struct_name.insert("F32Map".to_string());
    occupied_struct_name.insert("BoolMap".to_string());

    for n in state.operations.clone() {
        occupied_struct_name.insert(n.response_name.clone());
    }


    for (name,_) in root["components"]["schemas"].entries() {
        let raw_name = name.to_string();
        let mut name = state.struct_name_prefix.clone() + raw_name.clone().as_str();
        if name != capitalize_first_letter(name.as_str()) {
            name = capitalize_first_letter(name.as_str())
        }

        if !occupied_struct_name.contains(name.as_str()) {
            occupied_struct_name.insert(name.clone());
            state.struct_name_map.insert(raw_name, name);
            continue;
        }

        let base = name;
        let mut inc = 0;
        loop {
            let name_permutation = format!("{}{}", base.clone(), inc);
            if !occupied_struct_name.contains(name_permutation.as_str()) {
                occupied_struct_name.insert(name_permutation.clone());
                state.struct_name_map.insert(raw_name, name_permutation);
                break;
            }
            inc += 1;
        }
    }
}

fn show_args_and_exit(message: &str) {
    println!("{}", message);
    println!();
    println!("--help                         shows this message.");
    println!("--ffi-prefix <name>            prefix of all exported ffi fn's. default: \"\"");
    println!("--ffi-op-prefix <name>         prefix of all exported operations ffi fn's. default: \"\"");
    println!("--ffi-accessor-prefix <name>   prefix of all exported ffi struct getter/setter fn's. default: \"\"");
    println!("--struct-name-prefix <name>    prefix of struct names. default: \"\"");
    println!("--out <name>                   path to generated rust code. default: print to stdout");
    println!();
    println!("Sample usage:");
    println!("openapi-client-gen --ffi-prefix turbo_api --out src/api.rs schema.json");
    std::process::exit(-1);
}

fn poll_or_exit(q: &mut VecDeque<String>, message: &str) -> String {
    let arg = q.pop_front();
    if arg.is_none() {
        show_args_and_exit(&message);
    }

    arg.unwrap()
}

fn main() {
    let data = include_str!("rt.rs");
    let mut args: VecDeque<String> = std::env::args().collect();
    let mut state = State::default();
    let mut file;
    let file_path;
    let mut out = None;
    args.pop_front().expect("First arg is not path to executable???");
    loop {
        let arg = args.pop_front();
        if arg.is_none() {
            show_args_and_exit("Not enough arguments.");
        }
        let arg = arg.unwrap();
        match arg.to_lowercase().as_str() {
            "--help" => show_args_and_exit("Help:"),
            "--ffi-prefix" => state.ffi_prefix = poll_or_exit(&mut args, "--ffi-prefix expects more arguments"),
            "--ffi-op-prefix" => state.ffi_op_prefix = poll_or_exit(&mut args, "--ffi-op-prefix expects more arguments"),
            "--ffi-accessor-prefix" => state.ffi_accessor_prefix = poll_or_exit(&mut args, "--ffi-accessor-prefix expects more arguments"),
            "--struct-name-prefix" => state.struct_name_prefix = poll_or_exit(&mut args, "--struct-name-prefix expects more arguments"),
            "--out" =>  out = Some(poll_or_exit(&mut args, "--out expects more arguments")),
            _ => {
                if args.pop_front().is_some() {
                    show_args_and_exit("Too many arguments.");
                }

                file_path = arg.clone();
                file = File::open(arg.as_str()).expect(format!("Failed to open input file {}", arg).as_str());
                break;
            }
        }

    }

    let mut file_data = vec![];
    file.read_to_end(&mut file_data).expect(format!("Failed to read input file{}", file_path).as_str());
    let json_raw = String::from_utf8(file_data);
    if json_raw.is_err() {
        panic!("Input file {} is not utf-8", file_path);
    }
    let root = json::parse(json_raw.unwrap().as_str());
    if root.is_err() {
        panic!("Input file {} is not valid json", file_path);
    }
    let mut root = root.unwrap();

    sanitize_paths(&mut root);
    sanitize_request_bodies(&mut state, &mut root);
    sanitize_header(&mut state, &mut root);
    sanitize_schemas(&mut state, &mut root);

    collect_operations(&mut state, &root["paths"]);
    collect_struct_names(&mut state, &root);
    collect_map_types(&mut state, &root);

    let data = data.replace("extern \"C\" fn ", format!("extern \"C\" fn {}", state.ffi_prefix).as_str());
    state.push(data);
    state.push("\n");

    generate_model(&mut state, &root["components"]["schemas"]);
    generate_paths(&mut state, &root["paths"]);
    generate_ffi_maps(&mut state);

    if out.is_none() {
        io::stdout().write_all(state.main_buffer.as_slice()).unwrap();
        std::process::exit(0);
    }
    let out = out.unwrap();
    let mut file = File::create(out.as_str()).expect(format!("Failed to open output file for writing {}", out).as_str());
    file.set_len(0).expect(format!("Failed to truncate file {}", out).as_str());
    file.write_all(state.main_buffer.as_slice()).expect(format!("Failed to write to file {}", out).as_str());
}

fn copy_json_item<T: ToString>(root: &JsonValue, path: T) -> JsonValue {
    let str = path.to_string();
    if !str.starts_with("#/components/") {
        panic!("Could not find $ref {}", str);
    }
    let str = &str.to_string()[2..];
    let mut ele = root;

    for x in str.split("/") {
        ele = &ele[x];
    }

    ele.clone()
}

fn sanitize_request_bodies(state: &mut State, root: &mut JsonValue) {
    let mut x = 100i32;
    loop {
        x -= 1;
        if x < 0 {
            panic!("Too much recursion!")
        }
        sanitize_schemas(state, root);
        let old_state = root["components"]["requestBodies"].clone();
        for (name, value) in old_state.entries() {
            if !value["$ref"].is_null() {
                let path = value["$ref"].as_str();
                if path.is_none() {
                    panic!("#/compoments/requestBodies/{}->$ref is not a string", name)
                }

                let path = path.unwrap().to_string();
                if !path.starts_with("#/components/headers/") {
                    panic!("#/compoments/requestBodies/{}->$ref is not valid", name)
                }

                let copy = copy_json_item(root, path.as_str());
                if !copy.is_object() {
                    panic!("#/compoments/requestBodies/{}->$ref points to nowhere", name)
                }

                root["components"]["requestBodies"][name] = copy;
                continue;
            }
            for (content_type, content) in value["content"].entries() {
                match classify_schema(&content["schema"]) {
                    Schema::Invalid => panic!("#/compoments/requestBodies/{}/content/{} is invalid", name, content_type),
                    Schema::String | Schema::Int64 | Schema::Int32 | Schema::Any | Schema::Ref(_) |
                    Schema::StringArray | Schema::Int64Array | Schema::Int32Array | Schema::AnyArray |
                    Schema::AnyMap | Schema::StringMap | Schema::Int64Map | Schema::Int32Map
                    => {}
                    Schema::PolymorphicObjectImpl(_) | Schema::CompositeAllObjectImpl(_) | Schema::CompositeAnyObjectImpl(_) | Schema::CompositeOneObjectImpl(_) | Schema::ObjectImpl(_) | Schema::RefArray(_) | Schema::ImplArray(_) |  Schema::RefMap(_) | Schema::ImplMap(_)
                    => {
                        root["components"]["requestBodies"][name]["content"][content_type]["schema"] =
                            move_schema_implementation(&root["components"]["requestBodies"][name]["content"][content_type]["schema"].clone(),
                                                       format!("req_{}_{}", name.to_upper_camel_case(), get_name_for_content_type(content_type)).to_upper_camel_case(), root);
                    }
                    x=> panic!("#/compoments/requestBodies/{}/content/{} type is not yet implemented {}", name, content_type, x),
                }
            }
        }

        if root["components"]["requestBodies"] == old_state {
            break;
        }
    }
}

fn sanitize_header(state: &mut State, root: &mut JsonValue) {
    let mut x = 100i32;
    loop {
        x -= 1;
        if x < 0 {
            panic!("Too much recursion!")
        }
        sanitize_schemas(state, root);
        let old_state = root["components"]["headers"].clone();
        for (name, value) in old_state.entries() {
            if !value["$ref"].is_null() {
                let path = value["$ref"].as_str();
                if path.is_none() {
                    panic!("#/compoments/headers/{}->$ref is not a string", name)
                }

                let path = path.unwrap().to_string();
                if !path.starts_with("#/components/headers/") {
                    panic!("#/compoments/headers/{}->$ref is not valid", name)
                }

                let copy = copy_json_item(root, path.as_str());
                if !copy.is_object() {
                    panic!("#/compoments/headers/{}->$ref points to nowhere", name)
                }

                root["components"]["headers"][name] = copy;
                continue;
            }

            match classify_schema(&value["schema"]) {
                Schema::String => {}
                Schema::Int64 => {}
                Schema::Int32 => {}
                Schema::StringArray | Schema::Int64Array | Schema::Int32Array => {}
                Schema::StringMap | Schema::Int64Map | Schema::Int32Map => {}
                Schema::Any | Schema::AnyMap | Schema::AnyArray => {}
                Schema::Ref(referent) => {
                    match classify_schema(&root["components"]["schemas"][referent.as_str()]) {
                        Schema::Int32 | Schema::Int64 | Schema::String => {
                            root["components"]["headers"][name]["schema"] = root["components"]["schemas"][referent.as_str()].clone();
                        }
                        Schema::Invalid => panic!("#/compoments/headers/{} has invalid type {}", name, x),
                        _=> {}
                    }
                }
                Schema::ObjectImpl(_) | Schema::ImplArray(_) | Schema::RefArray(_) | Schema::CompositeAllObjectImpl(_) | Schema::RefMap(_) | Schema::ImplMap(_) => {
                    root["components"]["headers"][name]["schema"] = move_schema_implementation(&root["components"]["headers"][name]["schema"].clone(), format!("ComplexResponseHeader{}", name.to_upper_camel_case()), root);
                }
                x=> panic!("#/compoments/headers/{} has unsupported type {}", name, x)
            }
        }

        if root["components"]["headers"] == old_state {
            break;
        }
    }
}

fn sanitize_schemas(state: &mut State, root: &mut JsonValue) {
    let mut x = 100i32;
    loop {
        x-=1;
        if x < 0 {
            panic!("Too much recursion!")
        }
        let old_state = root["components"]["schemas"].clone();
        'outer: for (name, value) in old_state.entries() {
            match classify_schema(value) {
                Schema::Invalid => {
                    classify_schema(value);
                    panic!("Invalid schema {}", name);
                }
                Schema::Ref(referent) => {
                    root["components"]["schemas"][name] = root["components"]["schemas"][referent.as_str()].clone();
                }
                Schema::ImplArray(implementation) => {
                    let item_name = format!("Items{}", name);
                    root["components"]["schemas"][item_name.as_str()] = implementation.clone();
                    root["components"]["schemas"][name]["items"] = JsonValue::Object(Object::new());
                    root["components"]["schemas"][name]["items"]["$ref"] = JsonValue::String(format!("#/components/schemas/{}", item_name))
                }
                Schema::ImplMap(implementation) | Schema::ArrayMap(implementation) => {
                    let item_name = format!("Value{}", name);
                    root["components"]["schemas"][item_name.as_str()] = implementation.clone();
                    root["components"]["schemas"][name]["additionalProperties"] = JsonValue::Object(Object::new());
                    root["components"]["schemas"][name]["additionalProperties"]["$ref"] = JsonValue::String(format!("#/components/schemas/{}", item_name))
                }
                Schema::CompositeAnyObjectImpl(implementation) => {
                    for (idx, child) in implementation.members().enumerate() {
                        match classify_schema(child) {
                            Schema::Ref(_) => {}
                            Schema::ObjectImpl(inner_impl) => {
                                root["components"]["schemas"][name]["anyOf"][idx] = move_schema_implementation(&inner_impl, format!("Composite{}{}", name.to_upper_camel_case(), idx), root);
                            }
                            other => panic!("Composite contains {other} which is not yet implemented raw: {}", implementation.to_string()),
                        }
                    }
                }
                Schema::CompositeOneObjectImpl(implementation) => {
                    for (idx, child) in implementation.members().enumerate() {
                        match classify_schema(child) {
                            Schema::Ref(_) => {}
                            Schema::Constant(_) => {},
                            Schema::ObjectImpl(inner_impl) => {
                                root["components"]["schemas"][name]["oneOf"][idx] = move_schema_implementation(&inner_impl, format!("Composite{}{}", name.to_upper_camel_case(), idx), root);
                            }
                            other => panic!("Composite contains {other} which is not yet implemented raw: {}", implementation.to_string()),
                        }
                    }
                }
                Schema::CompositeAllObjectImpl(implementation) => {
                    let mut merged = JsonValue::Object(Object::new());
                    merged["type"] = "object".into();
                    merged["properties"] = JsonValue::Object(Object::new());
                    for (idx, child) in implementation.members().enumerate() {
                        match classify_schema(child) {
                            Schema::Ref(referent) => {
                                root["components"]["schemas"][name]["allOf"][idx] = root["components"]["schemas"][referent.as_str()].clone();
                                match classify_schema(&root["components"]["schemas"][name]["allOf"][idx]) {
                                    Schema::PolymorphicObjectImpl(_) => {
                                        if !state.poly_map.contains_key(&referent) {
                                            state.poly_map.insert(referent.clone(), HashSet::new());
                                        }

                                        state.poly_map.get_mut(&referent).unwrap().insert(name.to_string());
                                    }
                                    _=> (),

                                }
                                continue 'outer;
                            }
                            Schema::ObjectImpl(implementation) => {
                                for (field_name, field_schema) in implementation["properties"].entries() {
                                    merged["properties"][field_name] = field_schema.clone();
                                }
                            }
                            Schema::PolymorphicObjectImpl(implementation) => {
                                for (field_name, field_schema) in implementation["properties"].entries() {
                                    merged["properties"][field_name] = field_schema.clone();
                                }
                            }
                            other => panic!("Composite contains {other} which is not yet implemented raw: {}", implementation.to_string()),
                        }
                    }

                    root["components"]["schemas"][name] = merged;
                }
                Schema::ObjectImpl(implementation) => {
                    for (field_name, field_schema) in implementation["properties"].entries() {
                        match classify_schema(field_schema) {
                            Schema::Ref(_) | Schema::Any | Schema::Int32 | Schema::Int64 | Schema::String => {}
                            Schema::AnyMap | Schema::Int32Map | Schema::Int64Map | Schema::StringMap => {}
                            Schema::StringArray | Schema::Int32Array | Schema::Int64Array | Schema::AnyArray => {}
                            Schema::Double | Schema::DoubleArray | Schema::DoubleMap |
                            Schema::Float | Schema::FloatArray | Schema::FloatMap |
                            Schema::Boolean | Schema::BooleanMap | Schema::BooleanArray |
                            Schema::RefMap(_)
                            => {}
                            //Move the inner impl to its own thing.
                            Schema::ImplMap(implementation) | Schema::ArrayMap(implementation) => {
                                //TODO handle primitive arrays better, they should be inlined!
                                let item_name = format!("FV{}{}", name, field_name.to_upper_camel_case());
                                root["components"]["schemas"][name]["properties"][field_name]["additionalProperties"] = move_schema_implementation(
                                    &implementation, item_name, root,
                                );
                            }
                            //Move these to their own definition and give them their own name
                            Schema::RefArray(_) | Schema::ImplArray(_) | Schema::ObjectImpl(_) => {
                                let item_name = format!("F{}{}", name, field_name.to_upper_camel_case());
                                root["components"]["schemas"][name]["properties"][field_name] = move_schema_implementation(field_schema, item_name, root);
                            }
                            x => panic!("Field {} in object {} is invalid {}", field_name, name, x)
                        }
                    }
                }
                _=> {}
            }
        }

        if root["components"]["schemas"] == old_state {
            break;
        }
    }
}

fn sanitize_paths(root: &mut JsonValue) {
    if !root["components"].is_object() {
        root["components"] = JsonValue::Object(Object::new());
    }

    if !root["components"]["schemas"].is_object() {
        root["components"]["schemas"] = JsonValue::Object(Object::new());
    }

    for (_path_name, elem) in root["paths"].entries_mut() {
        //I want to worry about this later...
        elem.remove("summary");
        elem.remove("description");
        elem.remove("servers");
    }

    let root_clone = root.clone();

    for (_path_name, elem) in root["paths"].entries_mut() {
        if elem["$ref"].is_null() {
            continue;
        }
        let item = copy_json_item(&root_clone, elem["$ref"].as_str().unwrap());
        *elem = item;
    }




    for (_path_name, elem) in root["paths"].entries_mut() {
        let param = &mut elem["parameters"];
        if !param.is_null() {
            todo!("Sanitize parameter")
        }

        elem.remove("parameters");
    }

    for (path_name, elem) in root["paths"].entries_mut() {
        for (method, elem) in elem.entries_mut() {
            for param in elem["parameters"].members_mut() {
                if param["$ref"].is_null() {
                    continue;
                }

                let path = param["$ref"].as_str();
                if path.is_none() {
                    panic!("{} {} has a parameter with $ref that does not contain a valid string", path_name, method)
                }

                let path = path.unwrap().to_string();
                if !path.starts_with("#/components/parameters") {
                    panic!("{} {} has a parameter with $ref that does not contain a valid string", path_name, method)
                }
                let item = copy_json_item(&root_clone, path);
                *param = item;
            }
        }
    }



    let mut new_components : Vec<(String, JsonValue)> = Vec::new();

    for (path_name, elem) in root["paths"].entries_mut() {
        for (method, elem) in elem.entries_mut() {
            if !elem["requestBody"]["$ref"].is_null() {
                let path = elem["requestBody"]["$ref"].as_str();
                if path.is_none() {
                    panic!("$ref in requestBody is invalid {} {}", path_name, method)
                }

                let path = path.unwrap().to_string();
                if !path.starts_with("#/components/requestBodies/") {
                    panic!("$ref in requestBody is invalid {} {}", path_name, method)
                }

                let copy = copy_json_item(&root_clone, path.as_str());
                if copy.is_null() {
                    panic!("{} {} $ref leads to no where {}", path_name, method, path)
                }
                elem["requestBody"] = copy;
            }

            let operation_id = elem["operationId"].as_str();
            if operation_id.is_none() {
                panic!("operationId is missing {} {}", path_name, method)
            }
            let operation_id = operation_id.unwrap().to_string();

            for (content_type, elem) in elem["requestBody"]["content"].entries_mut() {
                if content_type == "application/json" {
                    match classify_schema(&elem["schema"]) {
                        Schema::Ref(_) => {}
                        _=> {
                            let name = format!("req_{}_{}", operation_id.to_snake_case(), get_name_for_content_type(content_type)).to_snake_case().to_upper_camel_case();
                            new_components.push((name.clone(), elem["schema"].clone()));
                            elem["schema"] = JsonValue::Object(Object::new());
                            elem["schema"]["$ref"] = JsonValue::String(format!("#/components/schemas/{}", name));
                        }
                    }
                }
            }
        }
    }

    for (path_name, elem) in root["paths"].entries_mut() {
        for (method, elem) in elem.entries_mut() {

            let operation_id = elem["operationId"].as_str();
            if operation_id.is_none() {
                panic!("operationId is missing {} {}", path_name, method)
            }
            let operation_id = operation_id.unwrap().to_string();

            for (code, elem) in elem["responses"].entries_mut() {
                if !elem["$ref"].is_null() {
                    let path = elem["$ref"].as_str();
                    if path.is_none() {
                        panic!("$ref in response is invalid {} {} {}", path_name, method, code)
                    }

                    let path = path.unwrap().to_string();
                    if !path.starts_with("#/components/responses/") {
                        panic!("$ref in response is invalid {} {} {}", path_name, method, code)
                    }

                    let copy = copy_json_item(&root_clone, path.as_str());
                    if copy.is_null() {
                        panic!("{} {} {} $ref leads to no where {}", path_name, method, code, path)
                    }
                    *elem = copy;
                }

                for (content_type, elem) in elem["content"].entries_mut() {
                    if content_type == "application/json" {
                        match classify_schema(&elem["schema"]) {
                            Schema::Ref(_) => {}
                            _=> {
                                let name = format!("rsp_{}_{}_{}", operation_id, code, get_name_for_content_type(content_type)).to_snake_case().to_upper_camel_case();
                                new_components.push((name.clone(), elem["schema"].clone()));
                                elem["schema"] = JsonValue::Object(Object::new());
                                elem["schema"]["$ref"] = JsonValue::String(format!("#/components/schemas/{}", name));
                            }
                        }
                    }
                }

            }
        }
    }

    for (name, elem) in new_components {
        root["components"]["schemas"][name.as_str()] = elem;
    }
}

fn collect_operations(state: &mut State, paths: &JsonValue) {
    for (path, element) in paths.entries() {
        for (method, element) in element.entries() {
            element["operationId"].as_str()
                .map(|a| a.to_string())
                .inspect(|e| state.operations.push(Operation::new(e.as_str(), path, method, element)));
        }
    }

    let mut to_remove = vec![];

    for op in &state.operations {
        for op2 in &state.operations {
            if op.counter == op2.counter {
                continue;
            }

            if op.name == op2.name {
                to_remove.push(op.counter);
                continue
            }

            if op.function_name == op2.function_name {
                to_remove.push(op.counter);
                continue
            }

            if op.response_name == op2.response_name {
                to_remove.push(op.counter);
                continue
            }
        }
    }

    state.operations.retain(|op| !to_remove.contains(&op.counter));
}

fn generate_any_object_model(state: &mut State, name: &str, object: &JsonValue) {
    let struct_name_string = state.struct_name_map.get(name).unwrap().clone();
    let struct_name = struct_name_string.as_str();

    state.push("\n#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]\n");
    state.push(format!("pub struct {} {{\n", struct_name));

    for n in object["anyOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(referent.as_str()).unwrap().clone();
                let field_name = referent.to_snake_case();
                state.push(format!("    pub {field_name}: O{ref_struct_name},\n"));
            },
            other => panic!("not implemented {other}"),
        }
    }

    state.push(format!("}}\n"));
    state.push(format!("option_wrapper!(O{}, {});\n", struct_name, struct_name));
    state.push(format!("as_request_body!({});\n", struct_name));

    generate_ffi_from_json(state, struct_name);
    generate_ffi_free_new(state, struct_name);

    state.push(format!("\nimpl Into<JsonValue> for {} {{\n", struct_name));
    state.push("    fn into(self) -> JsonValue {\n");
    state.push("        let mut base = JsonValue::Object(Object::new());\n\n");
    for n in object["anyOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let field_name = referent.to_snake_case();
                state.push(format!("        let current: JsonValue = self.{field_name}.into();\n"));
                state.push("        for (name, value) in current.entries() {\n");
                state.push("            base[name] = value.clone();\n");
                state.push("        }\n\n");
            }
            other => panic!("not implemented {other}"),
        }
    }

    state.push("        base\n");
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("\nimpl Into<JsonValue> for &{} {{\n", struct_name));
    state.push("    fn into(self) -> JsonValue {\n");
    state.push("        let mut base = JsonValue::Object(Object::new());\n\n");
    for n in object["anyOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let field_name = referent.to_snake_case();
                state.push(format!("        let current: JsonValue = (&self.{field_name}).into();\n"));
                state.push("        for (name, value) in current.entries() {\n");
                state.push("            base[name] = value.clone();\n");
                state.push("        }\n\n");
            }
            other => panic!("not implemented {other}"),
        }
    }

    state.push("        base\n");
    state.push("    }\n");
    state.push("}\n");


    //FROM &JsonValue
    state.push(format!("\nimpl TryFrom<&JsonValue> for {} {{\n", struct_name));
    state.push("    type Error = String;\n");
    state.push("    fn try_from(value: &JsonValue) -> Result<Self, String> {\n");
    state.push("        if value.is_null() {\n");
    state.push("            return Err(\"Non null expected\".to_string());\n");
    state.push("        }\n");
    state.push("        let mut count = 0;\n");
    state.push(format!("        let result = {} {{\n", struct_name));
    for n in object["anyOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(&referent).unwrap().clone();
                let field_name = referent.to_snake_case();
                state.push(format!("            {field_name}: O{ref_struct_name}::try_from(value).inspect(|_| count += 1).unwrap_or_default(),\n"))
            }
            other => panic!("not implemented {other}"),
        }
    }
    state.push(format!("        }};\n"));
    state.push("        if count == 0 {\n");
    state.push("            return Err(\"Invalid Schema\".to_string());\n");
    state.push("        }\n");
    state.push("        Ok(result)\n");
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));
}

fn generate_poly_object_model(state: &mut State, name: &str, object: &JsonValue) {
    let struct_name_string = state.struct_name_map.get(name).unwrap().clone();
    let struct_name = struct_name_string.as_str();

    let polys = state.poly_map.get(name).unwrap().clone();
    let discriminator = object["discriminator"]["propertyName"].as_str().unwrap();

    state.push("\n#[derive(Debug, Clone, Hash, PartialEq, Eq)]\n");
    state.push(format!("pub enum {} {{\n", struct_name));
    for n in polys.iter() {
        let poly_name = state.struct_name_map.get(n.as_str()).unwrap().clone();
        state.push(format!("    {poly_name}({poly_name}),\n"));
    }
    state.push("}\n");
    state.push(format!("option_wrapper!(O{}, {});\n", struct_name, struct_name));
    state.push(format!("as_request_body!({});\n", struct_name));

    state.push(format!("\nimpl Default for {} {{\n", struct_name));
    state.push("    fn default() -> Self {\n");
    for n in polys.iter() {
        let poly_name = state.struct_name_map.get(n.as_str()).unwrap().clone();
        state.push(format!("        {struct_name}::{poly_name}({poly_name}::default())\n"));
        break;
    }
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("\nimpl Into<JsonValue> for {} {{\n", struct_name));
    state.push("    fn into(self) -> JsonValue {\n");
    state.push("        match self {\n");
    for n in polys.iter() {
        let poly_name = state.struct_name_map.get(n.as_str()).unwrap().clone();
        state.push(format!("            {struct_name}::{poly_name}(value) => value.into(),\n"));
    }
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("\nimpl Into<JsonValue> for &{} {{\n", struct_name));
    state.push("    fn into(self) -> JsonValue {\n");
    state.push("        match self {\n");
    for n in polys.iter() {
        let poly_name = state.struct_name_map.get(n.as_str()).unwrap().clone();
        state.push(format!("            {struct_name}::{poly_name}(value) => {{\n"));
        state.push("                let mut result : JsonValue = value.into();\n");
        state.push(format!("                result[\"{discriminator}\"] = JsonValue::String(\"{n}\".to_string());\n"));
        state.push("                result\n");
        state.push("            }\n");
    }
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");


    state.push(format!("\nimpl TryFrom<&JsonValue> for {} {{\n", struct_name));
    state.push("    type Error = String;\n");
    state.push("    fn try_from(value: &JsonValue) -> Result<Self, String> {\n");
    state.push("        if value.is_null() {\n");
    state.push("            return Err(\"Non null expected\".to_string());\n");
    state.push("        }\n");

    state.push(format!("        let Some(discriminator) = value[\"{discriminator}\"].as_str() else {{\n"));
    state.push("            return Err(\"Discriminator is not a String\".to_string());\n");
    state.push("        };\n");
    state.push("        match discriminator {\n");
    for n in polys.iter() {
        let poly_name = state.struct_name_map.get(n.as_str()).unwrap().clone();
        state.push(format!("            \"{n}\" => Ok({struct_name}::{poly_name}({poly_name}::try_from(value)?)),\n"));
    }
    state.push("            other => Err(format!(\"Unexpected discriminator value {other}\"))\n");
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");

}

fn generate_one_object_model(state: &mut State, name: &str, object: &JsonValue) {
    let struct_name_string = state.struct_name_map.get(name).unwrap().clone();
    let struct_name = struct_name_string.as_str();

    state.push("\n#[derive(Debug, Clone, Hash, PartialEq, Eq)]\n");
    state.push(format!("pub enum {} {{\n", struct_name));

    for n in object["oneOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(referent.as_str()).unwrap().clone();
                state.push(format!("    {ref_struct_name}({ref_struct_name}),\n"));
            },
            Schema::Constant(constant) => {
                let const_name = "Const".to_string() + &constant.as_str().expect("Non String Constant").to_string().to_upper_camel_case();
                state.push(format!("    {const_name},\n"));
            }
            other => panic!("not implemented {other}"),
        }
    }
    state.push(format!("}}\n"));
    state.push(format!("option_wrapper!(O{}, {});\n", struct_name, struct_name));
    state.push(format!("as_request_body!({});\n", struct_name));

    generate_ffi_from_json(state, struct_name);
    generate_ffi_free_new(state, struct_name);

    state.push(format!("\nimpl Default for {} {{\n", struct_name));
    state.push("    fn default() -> Self {\n");
    for n in object["oneOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(referent.as_str()).unwrap().clone();
                state.push(format!("        Self::{ref_struct_name}({ref_struct_name}::default())\n"));
                break;
            },
            Schema::Constant(constant) => {
                let const_name = "Const".to_string() + &constant.as_str().expect("Non String Constant").to_string().to_upper_camel_case();
                state.push(format!("        Self::{const_name}\n"));
                break;
            }
            other => panic!("not implemented {other}"),
        }
    }
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("\nimpl Into<JsonValue> for {} {{\n", struct_name));
    state.push("    fn into(self) -> JsonValue {\n");
    state.push("        match self {\n");
    for n in object["oneOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(referent.as_str()).unwrap().clone();
                state.push(format!("            {struct_name}::{ref_struct_name}(value) => value.into(),\n"));
            }
            Schema::Constant(constant) => {
                let const_value = constant.as_str().expect("Non String Constant").to_string();
                let const_name = "Const".to_string() + &const_value.to_upper_camel_case();
                state.push(format!("            {struct_name}::{const_name} => JsonValue::String(\"{const_value}\".to_string()),\n"));
            }
            other => panic!("not implemented {other}"),
        }
    }
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("\nimpl Into<JsonValue> for &{} {{\n", struct_name));
    state.push("    fn into(self) -> JsonValue {\n");
    state.push("        match self {\n");
    for n in object["oneOf"].members() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(referent.as_str()).unwrap().clone();
                state.push(format!("            {struct_name}::{ref_struct_name}(value) => value.into(),\n"));
            }
            Schema::Constant(constant) => {
                let const_value = constant.as_str().expect("Non String Constant").to_string();
                let const_name = "Const".to_string() + &const_value.to_upper_camel_case();
                state.push(format!("            {struct_name}::{const_name} => JsonValue::String(\"{const_value}\".to_string()),\n"));
            }
            other => panic!("not implemented {other}"),
        }
    }
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("\nimpl TryFrom<&JsonValue> for {} {{\n", struct_name));
    state.push("    type Error = String;\n");
    state.push("    fn try_from(value: &JsonValue) -> Result<Self, String> {\n");
    state.push("        if value.is_null() {\n");
    state.push("            return Err(\"Non null expected\".to_string());\n");
    state.push("        }\n");

    let push_inner = |state: &mut State, idx: usize, schema_name: &str, schema: Schema| {
        if matches!(schema, Schema::Constant(_)) {
            return;
        }

        for (idx2, n2) in object["oneOf"].members().enumerate() {
            if idx >= idx2 {
                continue;
            }

            match classify_schema(n2) {
                Schema::Ref(referent) => {
                    let ref_struct_name2 = state.struct_name_map.get(&referent).unwrap().clone();
                    state.push(format!("            if {ref_struct_name2}::try_from(value).is_ok() {{\n"));
                    state.push(format!("                return Err(\"Both {schema_name} and {ref_struct_name2} match\".to_string())\n"));
                    state.push("            }\n");
                }
                Schema::Constant(constant) => {
                    let const_value = constant.as_str().expect("Non String Constant").to_string();
                    let const_name = "Const".to_string() + &const_value.to_upper_camel_case();
                    state.push(format!("            if value.as_str() == Some(\"{const_value}\") {{\n"));
                    state.push(format!("                return Err(\"Both {schema_name} and {const_name} match\".to_string())\n"));
                    state.push("            }\n");
                }
                other => panic!("not implemented {other}"),
            }
        }
    };

    for (idx, n) in object["oneOf"].members().enumerate() {
        match classify_schema(n) {
            Schema::Ref(referent) => {
                let ref_struct_name = state.struct_name_map.get(&referent).unwrap().clone();
                state.push(format!("        let result = {ref_struct_name}::try_from(value);\n"));
                state.push("        if let Ok(res) = result {\n");

                push_inner(state, idx, ref_struct_name.as_str(), classify_schema(n));

                state.push(format!("            return Ok({struct_name}::{ref_struct_name}(res));\n"));
                state.push("        }\n");
            }
            Schema::Constant(constant) => {
                let const_value = constant.as_str().expect("Non String Constant").to_string();
                let const_name = "Const".to_string() + &const_value.to_upper_camel_case();
                state.push(format!("        if value.as_str() == Some(\"{const_value}\") {{\n"));
                push_inner(state, idx, const_name.as_str(), classify_schema(n));
                state.push(format!("            return Ok({struct_name}::{const_name});\n"));
                state.push("        }\n");
            }
            other => panic!("not implemented {other}"),
        }
    }
    state.push("        Err(\"Invalid Schema\".to_string())\n");
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));
}

fn generate_model_object(state: &mut State, name: &str, object: &JsonValue) {

    //generate_option_wrapper(state, name);

    let struct_name_string = state.struct_name_map.get(name).unwrap().clone();
    let struct_name = struct_name_string.as_str();


    let field_name_map = escape_field_names(object["properties"].entries().map(|(a,_)| a.to_string()).collect());

    state.push("\n#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]\n");
    state.push(format!("pub struct {} {{\n", struct_name));

    generate_ffi_from_json(state, struct_name);
    generate_ffi_free_new(state, struct_name);


    for (prop_name, field_schema) in object["properties"].entries() {
        let field_name = field_name_map.get(prop_name).unwrap();
        let ffi_fn_name = format!("{}_{}", struct_name, field_name.to_snake_case());

        match classify_schema(field_schema) {
            Schema::String => {
                generate_string_ffi_getter_setter(state, struct_name, field_name, &ffi_fn_name);
                state.push(format!("    pub {}: OString,\n", field_name))
            }
            Schema::Int64 => {
                generate_simple_ffi_getter_setter(state, struct_name, field_name, "i64", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OI64,\n", field_name))
            }
            Schema::Int32 => {
                generate_simple_ffi_getter_setter(state, struct_name, field_name, "i32", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OI32,\n", field_name))
            }
            Schema::Any => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "AnyElement", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OAnyElement,\n", field_name))
            }
            Schema::Ref(referent) => {
                let referent = state.struct_name_map.get(&referent).unwrap().clone();
                generate_object_ffi_getter_setter(state, struct_name, field_name, referent.as_str(), ffi_fn_name.as_str());
                state.push(format!("    pub {}: O{},\n", field_name, referent))
            }
            Schema::RefMap(referent) => {
                let referent = state.struct_name_map.get(&referent).unwrap().clone();
                generate_object_ffi_getter_setter(state, struct_name, field_name, format!("Map<O{}>", referent).as_str(), ffi_fn_name.as_str());
                state.push(format!("    pub {}: OMap<O{}>,\n", field_name, referent))
            }
            Schema::StringArray => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "StringArray", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OStringArray,\n", field_name))
            }
            Schema::Int64Array => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "I64Array", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OI64Array,\n", field_name))
            }
            Schema::Int32Array => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "I32Array", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OI32Array,\n", field_name))
            }
            Schema::AnyArray => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "AnyElementArray", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OAnyElementArray,\n", field_name))
            }
            Schema::AnyMap => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "AnyElementMap", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OAnyElementMap,\n", field_name))
            }
            Schema::StringMap => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "StringMap", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OStringMap,\n", field_name))
            }
            Schema::Int64Map => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "I64Map", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OI64Map,\n", field_name))
            }
            Schema::Int32Map => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "I32Map", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OI32Map,\n", field_name))
            }
            Schema::Double => {
                generate_simple_ffi_getter_setter(state, struct_name, field_name, "f64", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OF64,\n", field_name))
            }
            Schema::DoubleArray => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "F64Array", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OF64Array,\n", field_name))
            }
            Schema::DoubleMap => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "F64Map", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OF64Map,\n", field_name))
            }
            Schema::Boolean => {
                generate_simple_ffi_getter_setter(state, struct_name, field_name, "bool", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OBool,\n", field_name))
            }
            Schema::BooleanMap => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "BoolMap", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OBoolMap,\n", field_name))
            }
            Schema::BooleanArray => {
                generate_object_ffi_getter_setter(state, struct_name, field_name, "BoolArray", ffi_fn_name.as_str());
                state.push(format!("    pub {}: OBoolArray,\n", field_name))
            }
            x=> panic!("Invalid schema {} {} {}", struct_name, prop_name, x),
        }
    }
    state.push(format!("}}\n"));
    state.push(format!("option_wrapper!(O{}, {});\n", struct_name, struct_name));
    state.push(format!("as_request_body!({});\n", struct_name));

    state.insert_ffi();

    state.push(format!("\nimpl Display for {} {{\n", struct_name));
    state.push("    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {\n");
    state.push("        Display::fmt(self.to_json_pretty().as_str(), f)\n");
    state.push("    }\n");
    state.push("}\n");

    //FROM &JsonValue
    state.push(format!("\nimpl TryFrom<&JsonValue> for {} {{\n", struct_name));
    state.push("    type Error = String;\n");
    state.push("    fn try_from(value: &JsonValue) -> Result<Self, String> {\n");
    state.push("        if !value.is_object() {\n");
    state.push("            return Err(\"Object expected\".to_string());\n");
    state.push("        }\n");
    state.push("        for (prop_name,_) in value.entries() {\n");
    state.push("            match prop_name {\n");
    for (prop_name, _) in object["properties"].entries() {
        state.push(format!("                \"{prop_name}\" => (),\n"));
    }
    state.push("                prop_name => return Err(format!(\"Unknown property {}\", prop_name))\n");
    state.push("            }\n");
    state.push("        }\n");
    state.push(format!("        Ok({} {{\n", struct_name));
    for (prop_name, field_schema) in object["properties"].entries() {
        let field_name = field_name_map.get(prop_name).unwrap();


        match classify_schema(field_schema) {
            Schema::String => {
                state.push(format!("            {field_name}: OString::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Int64 => {
                state.push(format!("            {field_name}: OI64::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Int32 => {
                state.push(format!("            {field_name}: OI32::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Any => {
                //TODO make this faster
                state.push(format!("            {field_name}: OAnyElement(value.entries()\n"));
                state.push(format!("                    .filter(|(name, _value)| *name == \"{prop_name}\")\n"));
                state.push("                    .map(|(_name, value)| value.clone().into())\n");
                state.push("                    .into_iter()\n");
                state.push("                    .next()),\n");
            }
            Schema::Ref(referent) => {
                let referent = state.struct_name_map.get(&referent).unwrap().clone();
                state.push(format!("            {field_name}: O{referent}::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::RefMap(_referent) => {
                state.push(format!("            {field_name}: OMap::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::StringArray => {
                state.push(format!("            {field_name}: OStringArray::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Int64Array => {
                state.push(format!("            {field_name}: OI64Array::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Int32Array => {
                state.push(format!("            {field_name}: OI32Array::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::AnyArray => {
                state.push(format!("            {field_name}: OAnyElementArray::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::AnyMap => {
                state.push(format!("            {field_name}: OAnyElementMap::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::StringMap => {
                state.push(format!("            {field_name}: OStringMap::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Int64Map => {
                state.push(format!("            {field_name}: OI64Map::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Int32Map => {
                state.push(format!("            {field_name}: OI32Map::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Double => {
                state.push(format!("            {field_name}: OF64::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::DoubleArray => {
                state.push(format!("            {field_name}: OF64Array::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::DoubleMap => {
                state.push(format!("            {field_name}: OF64Map::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::Boolean => {
                state.push(format!("            {field_name}: OBool::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::BooleanMap => {
                state.push(format!("            {field_name}: OBoolMap::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            Schema::BooleanArray => {
                state.push(format!("            {field_name}: OBoolArray::try_from(&value[\"{prop_name}\"])?,\n"))
            }
            x=> panic!("Invalid schema {} {}", prop_name, x),
        }
    }
    state.push(format!("        }})\n"));
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));


    //To JsonValue
    state.push(format!("\nimpl Into<JsonValue> for {} {{\n", struct_name));
    state.push(format!("    fn into(self) -> JsonValue {{\n"));
    state.push("        let mut inst = JsonValue::Object(Object::new()); \n".to_string());
    for (prop_name, field_schema) in object["properties"].entries() {
        let field_name = field_name_map.get(prop_name).unwrap();
        match classify_schema(field_schema) {
            Schema::String | Schema::Int64 | Schema::Int32 | Schema::Any | Schema::Ref(_) |
            Schema::StringArray | Schema::Int64Array | Schema::Int32Array | Schema::AnyArray |
            Schema::Boolean | Schema::BooleanMap | Schema::BooleanArray |
            Schema::Double | Schema::DoubleMap | Schema::DoubleArray |
            Schema::AnyMap | Schema::StringMap | Schema::Int64Map | Schema::Int32Map | Schema::RefMap(_) =>
                state.push(format!("        inst[\"{}\"] = self.{}.into();\n", prop_name, field_name)),
            x=> panic!("Invalid schema {} {} {}", struct_name, prop_name, x),
        }
    }
    state.push("        return inst;\n");
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));

    //To JsonValue
    state.push(format!("\nimpl Into<JsonValue> for &{} {{\n", struct_name));
    state.push(format!("    fn into(self) -> JsonValue {{\n"));
    state.push("        let mut inst = JsonValue::Object(Object::new()); \n".to_string());
    for (prop_name, field_schema) in object["properties"].entries() {
        let field_name = field_name_map.get(prop_name).unwrap();
        match classify_schema(field_schema) {
            Schema::String | Schema::Int64 | Schema::Int32 | Schema::Any | Schema::Ref(_) |
            Schema::StringArray | Schema::Int64Array | Schema::Int32Array | Schema::AnyArray |
            Schema::Boolean | Schema::BooleanMap | Schema::BooleanArray |
            Schema::Double | Schema::DoubleMap | Schema::DoubleArray |
            Schema::AnyMap | Schema::StringMap | Schema::Int64Map | Schema::Int32Map| Schema::RefMap(_) =>
                state.push(format!("        inst[\"{}\"] = (&(self.{})).into();\n", prop_name, field_name)),
            x => panic!("Invalid schema {} {} {}", struct_name, prop_name, x),
        }
    }
    state.push("        return inst;\n".to_string());
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));
}

fn generate_ffi_from_json<T: ToString>(state: &mut State, name: T) {
    let name = name.to_string();
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}_from_json(json: *const c_char) -> *mut {} {{\n", state.ffi_prefix, name, name));
    state.push_ffi("    if json.is_null() {\n");
    state.push_ffi("            ffi_abort(\"from_json(NULL)\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let json = CStr::from_ptr(json).to_str();\n");
    state.push_ffi("    if json.is_err() {\n");
    state.push_ffi("        return  std::ptr::null_mut();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let json = json.unwrap();\n");
    state.push_ffi(format!("    match {}::from_json(json) {{\n", name));
    state.push_ffi("        Err(_) =>  std::ptr::null_mut(),\n");
    state.push_ffi("        Ok(obj) => Box::into_raw(Box::new(obj))\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}_to_json(inst: *const {}, buffer: *mut c_char, len: *mut usize) -> bool {{\n", state.ffi_prefix, name, name));
    state.push_ffi("    if len.is_null() {\n");
    state.push_ffi("        ffi_abort(\"to_json called with len null pointer\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    match inst.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"to_json called with inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(inst) => {\n");
    state.push_ffi("            let json = inst.to_json();\n");
    state.push_ffi("            let bytes = json.as_bytes();\n");
    state.push_ffi("            if len.read_unaligned() < bytes.len()+1 {\n");
    state.push_ffi("                len.write_unaligned(bytes.len()+1);\n");
    state.push_ffi("                return false;\n");
    state.push_ffi("            }\n");
    state.push_ffi("            len.write_unaligned(bytes.len()+1);\n");
    state.push_ffi("            if buffer.is_null() {\n");
    state.push_ffi("                return true;\n");
    state.push_ffi("            }\n");
    state.push_ffi("            for (idx, ele) in bytes.iter().enumerate() {\n");
    state.push_ffi("                match *ele as c_char {\n");
    state.push_ffi("                    0 => buffer.wrapping_add(idx).write_unaligned(32),\n");
    state.push_ffi("                    e => buffer.wrapping_add(idx).write_unaligned(e)\n");
    state.push_ffi("                }\n");
    state.push_ffi("            }\n");
    state.push_ffi("            buffer.wrapping_add(bytes.len()).write_unaligned(0);\n");
    state.push_ffi("            return true;\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");
}

fn generate_ffi_map<T: ToString>(state: &mut State, name: T) {
    let name = name.to_string();
    let name = state.struct_name_map.get(name.as_str()).unwrap().clone();
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}Map_{}_new() -> *mut Map<O{}> {{\n", state.ffi_prefix, name, name));
    state.push_ffi("    Box::into_raw(Box::new(Map::default()))\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}Map_{}{}_free(inst: *mut Map<O{}>) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, name, name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi("        ffi_abort(\"free(NULL)\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    _=Box::from_raw(inst)\n");
    state.push_ffi("}\n");


    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}Map_{}_keys(inst: *const  Map<O{}>) -> *mut StringArray {{\n", state.ffi_prefix, name, name));
    state.push_ffi("    match inst.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"Map_keys was called with a inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(any_map) => {\n");
    state.push_ffi("            ffi_get_map_keys(any_map, \"Map_keys\")\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}Map_{}_remove(inst: *mut Map<O{}>, key: *const c_char) {{\n", state.ffi_prefix, name, name));
    state.push_ffi("    if key.is_null() {\n");
    state.push_ffi("        ffi_abort(\"Map_remove was called with a key null pointer\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let key = CStr::from_ptr(key).to_str();\n");
    state.push_ffi("    if key.is_err() {\n");
    state.push_ffi("        ffi_abort(\"Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let key = key.unwrap();\n");
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"Map_remove was called with a inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(map) => {\n");
    state.push_ffi("            if map.remove(key).is_none() {\n");
    state.push_ffi("                ffi_abort(format!(\"Map_remove was called with key {} that does not exists\", key));\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}Map_{}_set(inst: *mut Map<O{}>, key: *const c_char, value: *const {}) {{\n", state.ffi_prefix, name, name, name));
    state.push_ffi("    if key.is_null() {\n");
    state.push_ffi("        ffi_abort(\"Map_set was called with a key null pointer\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let key = CStr::from_ptr(key).to_str();\n");
    state.push_ffi("    if key.is_err() {\n");
    state.push_ffi("        ffi_abort(\"Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let key = key.unwrap();\n");
    state.push_ffi("    let the_new_value = match value.as_ref() {\n");
    state.push_ffi("        Some(value) => value.clone().into(),\n");
    state.push_ffi(format!("        None => O{}::default()\n", name));
    state.push_ffi("    };\n");
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"Map_set was called with a inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(map) => {\n");
    state.push_ffi("            if map.insert(key.to_string(), the_new_value).is_some() {\n");
    state.push_ffi("                ffi_abort(format!(\"Map_set was called with key {} that already exists\", key));\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}Map_{}_get(inst: *const Map<O{}>, key: *const c_char) -> *mut {} {{\n", state.ffi_prefix, name, name, name));
    state.push_ffi("    if key.is_null() {\n");
    state.push_ffi("        ffi_abort(\"Map_get was called with a key null pointer\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let key = CStr::from_ptr(key).to_str();\n");
    state.push_ffi("    if key.is_err() {\n");
    state.push_ffi("        ffi_abort(\"Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let key = key.unwrap();\n");
    state.push_ffi("    match inst.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"Map_get was called with a inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(map) => {\n");
    state.push_ffi("            let value = map.get(key);\n");
    state.push_ffi("            if value.is_none() {\n");
    state.push_ffi("                ffi_abort(format!(\"Map_get was called with key {} that does not exists\", key));\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("            match value.unwrap().0.as_ref() {\n");
    state.push_ffi("                Some(inner) => Box::into_raw(Box::new(inner.clone())),\n");
    state.push_ffi("                None => std::ptr::null_mut()\n");
    state.push_ffi("            }\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.insert_ffi();
}

fn generate_ffi_maps(state: &mut State) {
    for mtype in state.map_types.clone() {
        generate_ffi_map(state, mtype);
    }
}


fn generate_ffi_free_new<T: ToString>(state: &mut State, name: T) {
    let name = name.to_string();
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}_new() -> *mut {} {{\n", state.ffi_prefix, name, name));
    state.push_ffi(format!("    Box::into_raw(Box::new({}::default()))\n", name));
    state.push_ffi("}\n");
    generate_ffi_free(state, name);
}

fn generate_ffi_free<T: ToString>(state: &mut State, name: T) {
    let name = name.to_string();
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_free(inst: *mut {}) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, name, name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi("        ffi_abort(\"free(NULL)\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    _=Box::from_raw(inst)\n");
    state.push_ffi("}\n");
}

fn generate_string_ffi_getter_setter<A: ToString, B: ToString, C: ToString>(state: &mut State, name: A, field_name: B, ffi_fn_name: C) {
    let name = name.to_string();
    let field_name = field_name.to_string();
    let ffi_fn_name = ffi_fn_name.to_string();
    //FFI String getter
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_get(inst: *const {}, buffer: *mut c_char, len: *mut usize) -> bool {{\n", state.ffi_prefix, state.ffi_accessor_prefix, ffi_fn_name, name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_get with null inst pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let inst = inst.as_ref().unwrap();\n");
    state.push_ffi("    if len.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_get with null len pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi(format!("    if inst.{}.0.is_none() {{\n", field_name));
    state.push_ffi("        len.write_unaligned(0);\n");
    state.push_ffi("        return true;\n");
    state.push_ffi("    }\n");
    state.push_ffi(format!("    let bytes = inst.{}.0.as_ref().unwrap().as_bytes();\n", field_name));
    state.push_ffi("    if len.read_unaligned() < bytes.len()+1 {\n");
    state.push_ffi("        len.write_unaligned(bytes.len()+1);\n");
    state.push_ffi("        return false;\n");
    state.push_ffi("    }\n");
    state.push_ffi("    len.write_unaligned(bytes.len()+1);\n");
    state.push_ffi("    if buffer.is_null() {\n");
    state.push_ffi("        return true;\n");
    state.push_ffi("    }\n");
    state.push_ffi("    for (idx, ele) in bytes.iter().enumerate() {\n");
    state.push_ffi("        match *ele as c_char {\n");
    state.push_ffi("            0 => buffer.wrapping_add(idx).write_unaligned(32),\n");
    state.push_ffi("            e => buffer.wrapping_add(idx).write_unaligned(e)\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("    buffer.wrapping_add(bytes.len()).write_unaligned(0);\n");
    state.push_ffi("    return true;\n");
    state.push_ffi("}\n");

    //FFI STRING SETTER
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_set(inst: * mut {}, str: *const c_char) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, ffi_fn_name, name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_set with null inst pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let inst = inst.as_mut().unwrap();\n");
    state.push_ffi("    if str.is_null() {\n");
    state.push_ffi(format!("        inst.{}.0 = None;\n", field_name));
    state.push_ffi("        return;\n");
    state.push_ffi("    }\n");
    state.push_ffi("    match CStr::from_ptr(str).to_str() {\n");
    state.push_ffi(format!("        Ok(string) => inst.{}.0 = Some(string.to_string()),\n", field_name));
    state.push_ffi("        Err(_) => {\n");
    state.push_ffi(format!("            ffi_abort(\"called {}_set with string that is not valid 0 terminated utf-8. Pointer my be invalid?\");\n", ffi_fn_name));
    state.push_ffi("            unreachable!();\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");
}

fn generate_object_ffi_getter_setter<A: ToString,B: ToString,C: ToString,D: ToString>(state: &mut State, struct_name: A, field_name: B, simple_type_name: C, ffi_fn_name: D) {
    let struct_name = struct_name.to_string();
    let field_name = field_name.to_string();
    let simple_type_name = simple_type_name.to_string();
    let ffi_fn_name = ffi_fn_name.to_string();

    //FFI getter
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_get(inst: *const {}) -> *mut {} {{\n", state.ffi_prefix, state.ffi_accessor_prefix, ffi_fn_name, struct_name, simple_type_name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_get with null inst pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let inst = inst.as_ref().unwrap();\n");
    state.push_ffi(format!("    if inst.{}.0.is_none() {{\n", field_name));
    state.push_ffi("        return std::ptr::null_mut();\n");
    state.push_ffi("    }\n");
    state.push_ffi(format!("    return Box::into_raw(Box::new(inst.{}.0.as_ref().unwrap().clone().into()));\n", field_name));
    state.push_ffi("}\n");

    //FFI setter
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_set(inst: *mut {}, value: *const {}) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, ffi_fn_name, struct_name, simple_type_name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_set with null inst pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let inst = inst.as_mut().unwrap();\n");
    state.push_ffi(format!("    inst.{}.0 = value.as_ref().map(|a| a.clone());\n", field_name));
    state.push_ffi("}\n");
}
fn generate_simple_ffi_getter_setter<A: ToString,B: ToString,C: ToString,D: ToString>(state: &mut State, struct_name: A, field_name: B, simple_type_name: C, ffi_fn_name: D) {
    let struct_name = struct_name.to_string();
    let field_name = field_name.to_string();
    let simple_type_name = simple_type_name.to_string();
    let ffi_fn_name = ffi_fn_name.to_string();

    //FFI getter
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_get(inst: *const {}, is_null: *mut bool) -> {} {{\n", state.ffi_prefix, state.ffi_accessor_prefix, ffi_fn_name, struct_name, simple_type_name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_get with null inst pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let inst = inst.as_ref().unwrap();\n");
    state.push_ffi("    if !is_null.is_null() {\n");
    state.push_ffi(format!("        is_null.write_unaligned(inst.{}.0.is_none());\n", field_name));
    state.push_ffi("    }\n");
    state.push_ffi(format!("    return inst.{}.0.unwrap_or({}::default());\n", field_name, simple_type_name));
    state.push_ffi("}\n");

    //FFI setter
    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_set(inst: *mut {}, value: *const {}) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, ffi_fn_name, struct_name, simple_type_name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi(format!("        ffi_abort(\"called {}_set with null inst pointer\");\n", ffi_fn_name));
    state.push_ffi("        unreachable!();\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let inst = inst.as_mut().unwrap();\n");
    state.push_ffi("    if !value.is_null() {\n");
    state.push_ffi(format!("        inst.{}.0 = None;\n", field_name));
    state.push_ffi("        return;\n");
    state.push_ffi("    }\n");
    state.push_ffi(format!("    inst.{}.0 = Some(*value);\n", field_name));
    state.push_ffi("}\n");
}

fn escape_field_names<T: ToString>(field_properties: Vec<T>) -> HashMap<String, String> {
    let mut mapping = HashMap::new();
    let mut value_set: HashSet<String> = HashSet::new();
    for x in field_properties {
        let original = x.to_string();
        let mapped = original.clone()
            .replace("-", "_")
            .replace("/", "_")
            .replace(":", "_")
            .replace(".", "_")
            .replace(" ", "_")
            //TODO this is probably incomplete list of characters that are not permitted in field names
            .to_snake_case();
        let mut x = 0;
        loop {
            let mapped = match mapped.as_str() {
                "type" => "type0".to_string(),
                "let" => "let0".to_string(),
                "struct" => "struct0".to_string(),
                "const" => "const0".to_string(),
                "union" => "union0".to_string(),
                "fn" => "fn0".to_string(),
                "return" => "return0".to_string(),
                "request_body" => "request_body0".to_string(), //I cannot be asked to escape this
                //TODO more?
                _=> mapped.clone(),
            };

            if !value_set.contains(&mapped) {
                value_set.insert(mapped.clone());
                mapping.insert(original, mapped);
                break;
            }

            x += 1;
            let formatted = format!("{}{}", mapped.as_str(), x);
            if !value_set.contains(&formatted) {
                value_set.insert(formatted.clone());
                mapping.insert(original, formatted);
                break;
            }
        }

    }

    mapping
}

fn escape_content_type_for_result_enum<A: ToString, B: ToString>(code: A, content_type: B) -> String {
    let code = code.to_string().to_upper_camel_case();
    let is_numeric = code.chars().next().map(|c| c.is_numeric()).unwrap(); //TODO empty string???
    let content_type = get_name_for_content_type(content_type);
    if is_numeric {
        return format!("S{}{}", code, content_type)
    }

    format!("{}{}", code, content_type)
}

fn get_name_for_content_type<T: ToString>(content_type: T) -> String {
    let mut content_type = content_type.to_string().replace("/", "_").replace("-", "_").replace("*", "Any").to_upper_camel_case();

    if content_type == "ApplicationJson" {
        content_type = "Json".to_string();
    }
    if content_type == "TextPlain" {
        content_type = "Text".to_string();
    }

    content_type
}

fn get_name_from_ref(refname: &str) -> String {
    let mut type_name = &refname[1..];
    let slash = type_name.rfind('/');
    if slash.is_some() {
        type_name = &type_name[slash.unwrap()+1..];
    }

    type_name.to_string()
}

fn generate_dump_model_array(state: &mut State, name: &str, _array: &JsonValue, referent: String) {
    let struct_name_string = state.struct_name_map.get(name).unwrap().clone();
    let struct_name = struct_name_string.as_str();

    let referent_name_string = state.struct_name_map.get(&referent).unwrap().clone();
    let referent_name = referent_name_string.as_str();

    state.push("\n#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]\n".to_string());
    state.push(format!("pub struct {}(", struct_name));
    state.push(format!("pub Vec<O{}>);\n", referent_name));
    state.push(format!("option_wrapper!(O{}, {});\n", struct_name, struct_name));
    state.push(format!("as_request_body!({});\n", struct_name));

    state.push(format!("\nimpl Display for {} {{\n", struct_name));
    state.push("    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {\n");
    state.push("        Display::fmt(self.to_json_pretty().as_str(), f)\n");
    state.push("    }\n");
    state.push("}\n");

    state.push(format!("impl Deref for {} {{\n", struct_name));
    state.push(format!("type Target = Vec<O{}>;\n", referent_name));
    state.push("fn deref(&self) -> &Self::Target {\n");
    state.push("&self.0\n");
    state.push("}\n");
    state.push("}\n");
    state.push(format!("impl DerefMut for {} {{\n", struct_name));
    state.push("fn deref_mut(&mut self) -> &mut Self::Target {\n");
    state.push("&mut self.0\n");
    state.push("}\n");
    state.push("}\n");

    state.push(format!("\nimpl Into<JsonValue> for {} {{\n", struct_name));
    state.push(format!("    fn into(self) -> JsonValue {{\n"));
    state.push("        return JsonValue::Array(self.0.iter().map(|e| e.into()).collect());\n".to_string());
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));
    state.push(format!("\nimpl Into<JsonValue> for &{} {{\n", struct_name));
    state.push(format!("    fn into(self) -> JsonValue {{\n"));
    state.push("        return JsonValue::Array(self.0.iter().map(|e| e.into()).collect());\n".to_string());
    state.push(format!("    }}\n"));
    state.push(format!("}}\n"));

    state.push(format!("impl TryFrom<&JsonValue> for {} {{\n", struct_name));
    state.push("    type Error = String;\n");
    state.push("    fn try_from(value: &JsonValue) -> Result<Self, String> {\n".to_string());
    state.push("        match value {\n".to_string());
    state.push("            JsonValue::Array(vec) => {\n");
    state.push("                let mut data = Vec::new();\n");
    state.push("                for v in vec {\n");
    state.push("                    data.push(v.try_into()?);\n");
    state.push("                }\n");
    state.push("                Ok(Self(data))\n");
    state.push("            }\n");
    state.push("            _ => Err(\"Array expected\".to_string()),".to_string());
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_new() -> *mut {} {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name));
    state.push_ffi(format!("    Box::into_raw(Box::new({}::default()))\n", struct_name));
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_free(inst: *mut {}) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name));
    state.push_ffi("    if inst.is_null() {\n");
    state.push_ffi("        ffi_abort(\"free(NULL)\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    _=Box::from_raw(inst)\n");
    state.push_ffi("}\n");

    generate_ffi_from_json(state, struct_name);

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_size(inst: *mut {}) -> usize {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name));
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"called with a null instance pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(vec) => vec.len()\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_get(inst: *mut {}, idx: usize) -> *mut {} {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name, referent_name));
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"called with a null instance pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(vec) => {\n");
    state.push_ffi("            if idx >= vec.len() {\n");
    state.push_ffi("                ffi_abort(format!(\"index {} out of bounds for array size {}\", idx, vec.len()));\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("            if vec[idx].is_none() {\n");
    state.push_ffi("                return std::ptr::null_mut();\n");
    state.push_ffi("            }\n");
    state.push_ffi("            Box::into_raw(Box::new(vec[idx].as_ref().unwrap().clone()))\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_remove(inst: *mut {}, idx: usize) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name));
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"called with a null instance pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(vec) => {\n");
    state.push_ffi("            if idx >= vec.len() {\n");
    state.push_ffi("                ffi_abort(format!(\"index {} out of bounds for array size {}\", idx, vec.len()));\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("            vec.remove(idx);\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_set(inst: *mut {}, idx: usize, value: *const {}) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name, referent_name));
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"called with a null instance pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(vec) => {\n");
    state.push_ffi("            if idx >= vec.len() {\n");
    state.push_ffi("                ffi_abort(format!(\"index {} out of bounds for array size {}\", idx, vec.len()));\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("            vec[idx] = value.as_ref().map(|a| a.clone()).into();\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_add(inst: *mut {}, value: *const {}) {{\n", state.ffi_prefix, state.ffi_accessor_prefix, struct_name, struct_name, referent_name));
    state.push_ffi("    match inst.as_mut() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"called with a null instance pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(vec) => vec.push(value.as_ref().map(|a| a.clone()).into())");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.insert_ffi();
}

fn generate_model(state: &mut State, schema: &JsonValue) {
    for (name, element) in schema.entries() {

        let struct_name = state.struct_name_map.get(name).unwrap().clone();

        match classify_schema(element) {
            Schema::Any => {
                state.push(format!("pub type {} = AnyElement;\n", struct_name));
                state.push(format!("pub type O{} = OAnyElement;\n", struct_name));
            }
            Schema::BooleanArray => {
                state.push(format!("pub type {} = BoolArray;\n", struct_name));
                state.push(format!("pub type O{} = OBoolArray;\n", struct_name));
            }
            Schema::FloatArray => {
                state.push(format!("pub type {} = F32Array;\n", struct_name));
                state.push(format!("pub type O{} = OF32Array;\n", struct_name));
            }
            Schema::DoubleArray => {
                state.push(format!("pub type {} = F64Array;\n", struct_name));
                state.push(format!("pub type O{} = OF64Array;\n", struct_name));
            }
            Schema::StringArray => {
                state.push(format!("pub type {} = StringArray;\n", struct_name));
                state.push(format!("pub type O{} = OStringArray;\n", struct_name));
            }
            Schema::String => {
                state.push(format!("pub type {} = OString;\n", struct_name));
                state.push(format!("pub type O{} = OString;\n", struct_name));
            }
            Schema::Boolean => {
                state.push(format!("pub type {} = OBool;\n", struct_name));
                state.push(format!("pub type O{} = OBool;\n", struct_name));
            }
            Schema::Double => {
                state.push(format!("pub type {} = OF64;\n", struct_name));
                state.push(format!("pub type O{} = OF64;\n", struct_name));
            }
            Schema::DoubleMap => {
                state.push(format!("pub type {} = F64Map;\n", struct_name));
                state.push(format!("pub type O{} = OF64Map;\n", struct_name));
            }
            Schema::Float => {
                state.push(format!("pub type {} = OF32;\n", struct_name));
                state.push(format!("pub type O{} = OF32;\n", struct_name));
            }
            Schema::FloatMap => {
                state.push(format!("pub type {} = F32Map;\n", struct_name));
                state.push(format!("pub type O{} = OF32Map;\n", struct_name));
            }
            Schema::Int64 => {
                state.push(format!("pub type {} = OI64;\n", struct_name));
                state.push(format!("pub type O{} = OI64;\n", struct_name));
            }
            Schema::Int64Map => {
                state.push(format!("pub type {} = I64Map;\n", struct_name));
                state.push(format!("pub type O{} = OI64Map;\n", struct_name));
            }
            Schema::Int32 => {
                state.push(format!("pub type {} = OI32;\n", struct_name));
                state.push(format!("pub type O{} = OI32;\n", struct_name));
            }
            Schema::Int32Map => {
                state.push(format!("pub type {} = I32Map;\n", struct_name));
                state.push(format!("pub type O{} = OI32Map;\n", struct_name));
            }
            Schema::Int64Array => {
                state.push(format!("pub type {} = I64Array;\n", struct_name));
                state.push(format!("pub type O{} = OI64Array;\n", struct_name));
            }
            Schema::Int32Array => {
                state.push(format!("pub type {} = I32Array;\n", struct_name));
                state.push(format!("pub type O{} = OI32Array;\n", struct_name));
            }
            Schema::RefMap(referent) => {
                let referent = state.struct_name_map.get(&referent).unwrap().clone();
                state.push(format!("pub type {} = Map<{}>;\n", struct_name, referent));
                state.push(format!("pub type O{} = OMap<{}>;\n", struct_name, referent));
            }
            Schema::ObjectImpl(_) => {
                generate_model_object(state, name, element);
            }
            Schema::CompositeAnyObjectImpl(_) => {
                generate_any_object_model(state, name, element);
            }
            Schema::CompositeOneObjectImpl(_) => {
                generate_one_object_model(state, name, element);
            }
            Schema::RefArray(referent) => {
                generate_dump_model_array(state, name, element, referent);
            }
            Schema::PolymorphicObjectImpl(_) => {
                generate_poly_object_model(state, name, element);
            }
            x => panic!("Invalid schema {} {}", name, x)
        }
    }
}
fn generate_operation(state: &mut State, operation: &Operation) {

    let desc = &operation.element;

    let responses = &desc["responses"];
    if responses.is_object() {
        generate_operation_response_header(state, operation, responses);
    }

    generate_operation_response_enum(state, &operation, responses);

    let param = match &desc["parameters"] {
        JsonValue::Array(x) => x.clone(),

        _=> Vec::new(),
    };


    let field_name_map = escape_field_names(param.iter().map(|a| a["name"].as_str()).filter(|a| a.is_some()).map(|a| a.unwrap().to_string()).collect());



    state.push_path("\n    #[cfg(feature = \"blocking\")]\n#[cfg(not(target_arch = \"wasm32\"))]\n");
    state.push_path(format!("    pub fn {}(&self", operation.function_name));
    state.push_async_path("\n    #[cfg(any(feature = \"async\", target_arch = \"wasm32\"))]\n");
    state.push_async_path(format!("    pub async fn {}(&self", operation.async_function_name));

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}(api: *const ApiClient", state.ffi_prefix, state.ffi_op_prefix, operation.function_name));

    for param_desc in &param {
        let param_name_raw = param_desc["name"].as_str();
        if param_name_raw.is_none() {
            continue;
        }
        let param_name_raw = param_name_raw.unwrap().to_string();
        let param_name = field_name_map.get(&param_name_raw).unwrap();
        let (param_type, path_param, ffi_param_type) = match classify_schema(&param_desc["schema"]) {
            Schema::String => ("OString", "impl PathParam<String>", "*const c_char"),
            Schema::Int64 => ("OI64", "impl PathParam<i64>", "*const i64"),
            Schema::Int32 => ("OI32", "impl PathParam<i32>", "*const i32"),
            Schema::Double => ("OF64", "impl PathParam<f64>", "*const f64"),
            Schema::Boolean => ("OBool", "impl PathParam<bool>", "*const bool"),
            Schema::StringArray => ("&OStringArray", "impl PathParam<StringArray>", "*const StringArray"),
            other => panic!("{other} {} {} param type not supported yet", operation.name, param_name_raw),
        };

        if param_desc["in"].as_str() == Some("path") {
            state.push_path(format!(", {}: {}", param_name, path_param));
            state.push_async_path(format!(", {}: {}", param_name, path_param));
        } else {
            state.push_path(format!(", {}: {}", param_name, param_type));
            state.push_async_path(format!(", {}: {}", param_name, param_type));
        }

        state.push_ffi(format!(", {}: {}", param_name, ffi_param_type));
    }


    let mut string_entity = None;
    let mut json_entity = None;
    let mut stream_entity = None;
    let mut request_body_content_type = None;
    match &desc["requestBody"] {
        JsonValue::Null => {}
        JsonValue::Object(body) => {
            for (content_type, content_type_ref) in body["content"].entries() {
                request_body_content_type = Some(content_type.to_string());
                    match content_type {
                    "application/json" => {
                        match classify_schema(&content_type_ref["schema"]) {
                            Schema::Ref(ref_name) => {
                                let ref_name = state.struct_name_map.get(ref_name.as_str()).unwrap().clone();
                                state.push_path(format!(", request_body: impl RequestBody<{}>", ref_name));
                                state.push_async_path(format!(", request_body: impl RequestBody<{}>", ref_name));
                                state.push_ffi(format!(", request_body: *const {}", ref_name));
                                json_entity = Some("request_body");
                            }
                            _=> panic!("{} request body type not supported for yet application/json", operation.name),
                        }
                    },
                    "text/plain" => {
                        match classify_schema(&content_type_ref["schema"]) {
                            Schema::String => {
                                state.push_path(", request_body: OString");
                                state.push_async_path(", request_body: OString");
                                state.push_ffi(", request_body: *const c_char");
                                string_entity = Some("request_body");
                            }
                            _=> panic!("{} request body type not supported for yet text/plain", operation.name),
                        }
                    }
                    _=> {
                        state.push_path(", request_body: OStream");
                        state.push_async_path(", request_body: OStream");
                        state.push_ffi(", request_body: *const Stream");
                        stream_entity = Some("request_body");
                    }
                }
            }
        }
        _=> panic!("{} operation has invalid requestBody attribute", operation.name),
    };

    state.push_ffi(format!(", success_result: *mut *mut {}, error_result: *mut *mut ApiError) -> bool {{\n", operation.response_name));
    state.push_ffi("    if success_result.is_null() {\n");
    state.push_ffi("        ffi_abort(\"success_result is null\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    if error_result.is_null() {\n");
    state.push_ffi("        ffi_abort(\"error_result is null\");\n");
    state.push_ffi("        unreachable!()\n");
    state.push_ffi("    }\n");
    state.push_ffi("    let api = match api.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"api is null\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(api) => api,\n");
    state.push_ffi("    };\n");

    let mut param_call : String = String::new();

    for param_desc in &param {
        let param_name_raw = param_desc["name"].as_str();
        if param_name_raw.is_none() {
            continue;
        }
        let param_name_raw = param_name_raw.unwrap().to_string();
        let param_name = field_name_map.get(&param_name_raw).unwrap();
        match classify_schema(&param_desc["schema"]) {
            Schema::String => {
                state.push_ffi(format!("    let {} = if !{}.is_null() {{\n", param_name, param_name));
                state.push_ffi(format!("        match CStr::from_ptr({}).to_str() {{\n", param_name));
                state.push_ffi("            Ok(str) => OString::from(str),\n");
                state.push_ffi("            Err(_) => {\n");
                state.push_ffi("                ffi_abort(\"string is not valid utf-8\");\n");
                state.push_ffi("                unreachable!()\n");
                state.push_ffi("            }\n");
                state.push_ffi("        }\n");
                state.push_ffi("    } else {\n");
                state.push_ffi("        OString::default()\n");
                state.push_ffi("    };\n");
                param_call += param_name;
                param_call += ", ";
            }
            Schema::Int64 => {
                state.push_ffi(format!("    let {} = match {}.as_ref() {{\n", param_name, param_name));
                state.push_ffi("        None => OI64::default(),\n");
                state.push_ffi("        Some(data) => OI64::from(*data)\n");
                state.push_ffi("    };\n");
                param_call += param_name;
                param_call += ", ";
            }
            Schema::Int32 => {
                state.push_ffi(format!("    let {} = match {}.as_ref() {{\n", param_name, param_name));
                state.push_ffi("        None => OI32::default(),\n");
                state.push_ffi("        Some(data) => OI32::from(*data)\n");
                state.push_ffi("    };\n");
                param_call += param_name;
                param_call += ", ";
            }
            Schema::Double => {
                state.push_ffi(format!("    let {} = match {}.as_ref() {{\n", param_name, param_name));
                state.push_ffi("        None => OF64::default(),\n");
                state.push_ffi("        Some(data) => OF64::from(*data)\n");
                state.push_ffi("    };\n");
                param_call += param_name;
                param_call += ", ";
            }
            Schema::Boolean => {
                state.push_ffi(format!("    let {} = match {}.as_ref() {{\n", param_name, param_name));
                state.push_ffi("        None => OBool::default(),\n");
                state.push_ffi("        Some(data) => OBool::from(*data)\n");
                state.push_ffi("    };\n");
                param_call += param_name;
                param_call += ", ";
            }
            Schema::StringArray => {
                state.push_ffi(format!("    let {} = match {}.as_ref() {{\n", param_name, param_name));
                state.push_ffi("        None => OStringArray::default(),\n");
                state.push_ffi("        Some(data) => OStringArray::from(data.clone())\n");
                state.push_ffi("    };\n");
                param_call += "&";
                param_call += param_name;
                param_call += ", ";
            }
            other => panic!("{other} {} {} param type not supported yet", operation.name, param_name_raw),
        };


    }

    match &desc["requestBody"] {
        JsonValue::Null => {}
        JsonValue::Object(body) => {
            for (content_type, content_type_ref) in body["content"].entries() {
                request_body_content_type = Some(content_type.to_string());
                match content_type {
                    "application/json" => {
                        match classify_schema(&content_type_ref["schema"]) {
                            Schema::Ref(ref_name) => {
                                let ref_name = state.struct_name_map.get(ref_name.as_str()).unwrap().clone();
                                state.push_ffi("    let request_body = match request_body.as_ref() {\n");
                                state.push_ffi(format!("        None => O{}::default(),\n", ref_name));
                                state.push_ffi(format!("        Some(request_body) => O{}::from(request_body.clone())\n", ref_name));
                                state.push_ffi("    };\n");
                                param_call += "&request_body";
                            }
                            _=> panic!("{} request body type not supported for yet application/json", operation.name),
                        }
                    },
                    "text/plain" => {
                        match classify_schema(&content_type_ref["schema"]) {
                            Schema::String => {
                                state.push_ffi("    let request_body = if !request_body.is_null() {\n");
                                state.push_ffi("        match CStr::from_ptr(request_body).to_str() {\n");
                                state.push_ffi("            Ok(str) => OString::from(str),\n");
                                state.push_ffi("            Err(_) => {\n");
                                state.push_ffi("                ffi_abort(\"string is not valid utf-8\");\n");
                                state.push_ffi("                unreachable!()\n");
                                state.push_ffi("            }\n");
                                state.push_ffi("        }\n");
                                state.push_ffi("    } else {\n");
                                state.push_ffi("        OString::default()\n");
                                state.push_ffi("    };\n");
                                param_call += "request_body";
                            }
                            _=> panic!("{} request body type not supported for yet text/plain", operation.name),
                        }
                    }
                    _=> {
                        param_call += "request_body";
                        state.push_ffi("    let request_body = match request_body.as_ref() {\n");
                        state.push_ffi("        None => OStream::default(),\n");
                        state.push_ffi("        Some(data) => OStream(Some(data.clone()))\n");
                        state.push_ffi("    };\n");
                    }
                }
            }
        }
        _=> panic!("{} operation has invalid requestBody attribute", operation.name),
    };

    if param_call.ends_with(", ") {
        param_call = param_call.as_str()[0..(param_call.len()-2)].to_string();
    }

    state.push_ffi(format!("    match api.{}({}) {{\n", operation.function_name, param_call));
    state.push_ffi("        Ok(succ) => {\n");
    state.push_ffi("            success_result.write_unaligned(Box::into_raw(Box::new(succ)));\n");
    state.push_ffi("            true\n");
    state.push_ffi("        },\n");
    state.push_ffi("        Err(err) => {\n");
    state.push_ffi("            error_result.write_unaligned(Box::into_raw(Box::new(err)));\n");
    state.push_ffi("            false\n");
    state.push_ffi("        },\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");
    state.insert_ffi();

    state.push_path(format!(") -> Result<{}, ApiError> {{\n", operation.response_name));
    state.push_path(format!("        let request = self.request_customizer.customize_request_blocking(\"{}\", &self.client_blocking, ApiRequestBuilder::default()\n", operation.name.as_str()));
    state.push_path(format!("            .method(\"{}\")\n", operation.method));
    state.push_path(format!("            .path(\"{}\".to_string())\n", operation.path));

    state.push_async_path(format!(") -> Result<{}, ApiError> {{\n", operation.response_name));
    state.push_async_path(format!("        let request = self.request_customizer.customize_request_async(\"{}\", &self.client_async, ApiRequestBuilder::default()\n", operation.name.as_str()));
    state.push_async_path(format!("            .method(\"{}\")\n", operation.method));
    state.push_async_path(format!("            .path(\"{}\".to_string())\n", operation.path));

    if json_entity.is_some() {
        state.push_path(format!("            .entity_json({})\n", json_entity.unwrap()));
        state.push_async_path(format!("            .entity_json({})\n", json_entity.unwrap()));
    }
    if stream_entity.is_some() {
        state.push_path(format!("            .entity_stream({})\n", stream_entity.unwrap()));
        state.push_async_path(format!("            .entity_stream({})\n", stream_entity.unwrap()));
    }
    if string_entity.is_some() {
        state.push_path(format!("            .entity_string({})\n", string_entity.unwrap()));
        state.push_async_path(format!("            .entity_string({})\n", string_entity.unwrap()));
    }
    if request_body_content_type.is_some() {
        state.push_path(format!("            .set_header(\"Content-Type\", \"{}\")?\n", request_body_content_type.as_ref().unwrap()));
        state.push_async_path(format!("            .set_header(\"Content-Type\", \"{}\")?\n", request_body_content_type.as_ref().unwrap()));
    }

    for param_desc in &param {
        let param_name = param_desc["name"].as_str();
        if param_name.is_none() {
            continue;
        }

        let raw_param_name = param_name.unwrap().to_string();

        let param_name = field_name_map.get(&raw_param_name).unwrap();

        match param_desc["in"].as_str() {
            Some("path") => {
                match param_desc["style"].as_str() {
                    None | Some("simple") => {
                        if param_desc["explode"].as_bool().unwrap_or_default() {
                            state.push_path(format!("            .add_path_param(\"{}\", {}.to_path_param_simple_explode().unwrap_or(String::default()))\n", raw_param_name, param_name));
                            state.push_async_path(format!("            .add_path_param(\"{}\", {}.to_path_param_simple_explode().unwrap_or(String::default()))\n", raw_param_name, param_name));
                        } else {
                            state.push_path(format!("            .add_path_param(\"{}\", {}.to_path_param_simple().unwrap_or(String::default()))\n", raw_param_name, param_name));
                            state.push_async_path(format!("            .add_path_param(\"{}\", {}.to_path_param_simple().unwrap_or(String::default()))\n", raw_param_name, param_name));
                        }
                    }
                    Some("label") => {
                        if param_desc["explode"].as_bool().unwrap_or_default() {
                            state.push_path(format!("            .add_path_param(\"{}\", {}.to_path_param_label_explode().unwrap_or(String::default()))\n", raw_param_name, param_name));
                            state.push_async_path(format!("            .add_path_param(\"{}\", {}.to_path_param_label_explode().unwrap_or(String::default()))\n", raw_param_name, param_name));
                        } else {
                            state.push_path(format!("            .add_path_param(\"{}\", {}.to_path_param_label().unwrap_or(String::default()))\n", raw_param_name, param_name));
                            state.push_async_path(format!("            .add_path_param(\"{}\", {}.to_path_param_label().unwrap_or(String::default()))\n", raw_param_name, param_name));
                        }
                    }
                    Some("matrix") => {
                        if param_desc["explode"].as_bool().unwrap_or_default() {
                            state.push_path(format!("            .add_path_param(\"{}\", {}.to_path_param_matrix_explode({raw_param_name}).unwrap_or(String::default()))\n", raw_param_name, param_name));
                            state.push_async_path(format!("            .add_path_param(\"{}\", {}.to_path_param_matrix_explode({raw_param_name}).unwrap_or(String::default()))\n", raw_param_name, param_name));
                        } else {
                            state.push_path(format!("            .add_path_param(\"{}\", {}.to_path_param_matrix({raw_param_name}).unwrap_or(String::default()))\n", raw_param_name, param_name));
                            state.push_async_path(format!("            .add_path_param(\"{}\", {}.to_path_param_matrix({raw_param_name}).unwrap_or(String::default()))\n", raw_param_name, param_name));
                        }
                    }
                    Some(other) => panic!("unsupported param style {other}"),
                }
            }
            Some("query") => {
                match classify_schema(&param_desc["schema"]) {
                    Schema::String | Schema::Int64 | Schema::Int32 | Schema::Double | Schema::Boolean => {
                        state.push_path(format!("            .add_optional_query(\"{}\", {}.0.as_ref())\n", raw_param_name, param_name));
                        state.push_async_path(format!("            .add_optional_query(\"{}\", {}.0.as_ref())\n", raw_param_name, param_name));
                    },
                    Schema::StringArray => {
                        state.push_path(format!("            .add_string_array_query(\"{}\", {})\n", raw_param_name, param_name));
                        state.push_async_path(format!("            .add_string_array_query(\"{}\", {})\n", raw_param_name, param_name));
                    }
                    _=> panic!("{} {} param type not supported yet for query parameters", operation.name, raw_param_name),
                };
            }
            Some("header") => {
                match classify_schema(&param_desc["schema"]) {
                    Schema::String | Schema::Int64 | Schema::Int32 | Schema::Double | Schema::Boolean => {
                        state.push_path(format!("            .add_optional_header(\"{}\", {}.0.as_ref())?\n", raw_param_name, param_name));
                        state.push_async_path(format!("            .add_optional_header(\"{}\", {}.0.as_ref())?\n", raw_param_name, param_name));
                    },
                    _=> panic!("{} {} param type not supported yet for header parameters", operation.name, raw_param_name),
                };
            }
            Some("cookie") => {
                todo!("cookie parameters not yet implemented")
            }
            Some(_other) => panic!("{} {} invalid parameter in type", operation.name, raw_param_name),
            _ => continue,
        }


    }

    state.push_path("            )?\n");
    state.push_path("            .build_blocking(&self.base_url, &self.client_blocking)?;\n\n");
    state.push_path("        let request_headers = request.headers().clone();\n");
    state.push_path("        let request_url = request.url().clone();\n\n");
    state.push_path("        let response = self.client_blocking.execute(request)?;\n");
    state.push_path("        let status = response.status();\n");
    state.push_path("        let response_headers = response.headers().clone();\n");
    state.push_path(format!("        let response = match self.response_customizer.customize_response_blocking(\"{}\", &self.client_blocking, &request_url, &request_headers, response)? {{\n", operation.name.as_str()));
    state.push_path("            either::Either::Left(response) => response,\n");
    state.push_path(format!("            either::Either::Right(custom) => return Ok({}::Custom(custom, status, response_headers)),\n", operation.response_name));
    state.push_path("        };\n\n");
    state.push_path("        let content_type = get_content_type(&response_headers);\n");
    state.push_path("        match status.as_u16() {\n");

    state.push_async_path("            )?\n");
    state.push_async_path("            .build_async(&self.base_url, &self.client_async).await?;\n\n");
    state.push_async_path("        let request_headers = request.headers().clone();\n");
    state.push_async_path("        let request_url = request.url().clone();\n\n");
    state.push_async_path("        let response = self.client_async.execute(request).await?;\n");
    state.push_async_path("        let status = response.status();\n");
    state.push_async_path("        let response_headers = response.headers().clone();\n");
    state.push_async_path(format!("        let response = match self.response_customizer.customize_response_async(\"{}\", &self.client_async, &request_url, &request_headers, response).await {{\n", operation.name.as_str()));
    state.push_async_path("            either::Either::Left(response) => response,\n");
    state.push_async_path(format!("            either::Either::Right(custom) => return Ok({}::Custom(custom?, status, response_headers)),\n", operation.response_name));
    state.push_async_path("        };\n\n");
    state.push_async_path("        let content_type = get_content_type(&response_headers);\n");
    state.push_async_path("        match status.as_u16() {\n");

    if responses.is_object() {
        for (code, elem) in responses.entries() {
            let mut code_in_match_arm = code;
            if code_in_match_arm == "default" {
                code_in_match_arm = "_";
            } else {
                if u16::from_str_radix(code, 10).is_err() {
                    continue;
                }
            }

            let mut content = false;
            state.push_path(format!("            {} => match content_type {{\n", code_in_match_arm));
            state.push_async_path(format!("            {} => match content_type {{\n", code_in_match_arm));
            for (content_type, _relem) in elem["content"].entries() {
                content = true;
                state.push_path(format!("                Some(b\"{}\") => {{\n", content_type));
                state.push_async_path(format!("                Some(b\"{}\") => {{\n", content_type));
                let enum_type = escape_content_type_for_result_enum(code, content_type);
                match content_type {
                    "application/json" => {
                        state.push_path("                    let text = response.text()?;\n");
                        state.push_path("                    let json = json::parse(text.as_str());\n");
                        state.push_path("                    match json {\n");
                        state.push_path("                        Err(json_err) => Err(ApiError::JsonError(json_err, request_url, request_headers, status, response_headers, text)),\n");
                        state.push_path(format!("                        Ok(json) => Ok({}::{}((&json).try_into().map_err(|e| ApiError::UnexepectedJsonData(json, request_headers, status, response_headers, e))?", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_path(", response_headers.into()");
                        }
                        state.push_path("))\n                    }\n");
                        state.push_path("                },\n");

                        state.push_async_path("                    let text = response.text().await?;\n");
                        state.push_async_path("                    let json = json::parse(text.as_str());\n");
                        state.push_async_path("                    if json.is_err() {\n");
                        state.push_async_path("                        Err(ApiError::JsonError(json.unwrap_err(), request_url, request_headers, status, response_headers, text))\n");
                        state.push_async_path("                    } else {\n");
                        state.push_async_path("                        let json = json.unwrap();\n");
                        state.push_async_path(format!("                        Ok({}::{}((&json).try_into().map_err(|e| ApiError::UnexepectedJsonData(json, request_headers, status, response_headers, e))?", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_async_path(", response_headers.into()");
                        }
                        state.push_async_path("))\n                    }\n");
                        state.push_async_path("                },\n");
                    },
                    "text/plain" => {
                        state.push_path("                    let text = response.text()?;\n");
                        state.push_path(format!("       \
                                         Ok({}::{}(OString::from(text)", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_path(", response_headers.into()");
                        }
                        state.push_path("))\n                },\n");

                        state.push_async_path("                    let text = response.text().await?;\n");
                        state.push_async_path(format!("       \
                                         Ok({}::{}(OString::from(text)", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_async_path(", response_headers.into()");
                        }
                        state.push_async_path("))\n                },\n");
                    }
                    _ => {
                        state.push_path(format!("                    Ok({}::{}((Box::new(response) as Box<dyn io::Read+Send>).into()", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_path(", response_headers.into()");
                        }
                        state.push_path("))\n                },\n");
                        state.push_async_path("                    #[cfg(not(target_arch = \"wasm32\"))]\n");
                        state.push_async_path(format!("                    return Ok({}::{}(response.into()", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_async_path(", response_headers.into()");
                        }
                        state.push_async_path("));\n");
                        state.push_async_path("                    #[cfg(target_arch = \"wasm32\")]\n");
                        state.push_async_path(format!("                    return Ok({}::{}(response.bytes().await?.to_vec().into()", operation.response_name.as_str(), enum_type));
                        if elem["headers"].is_object() {
                            state.push_async_path(", response_headers.into()");
                        }
                        state.push_async_path("));\n");
                        state.push_async_path("                },\n");
                    }
                }
            }

            if !content {
                state.push_path(format!("                None => Ok({}::NoContent{}(", operation.response_name.as_str(), code));
                if elem["headers"].is_object() {
                    state.push_path(format!(" {}{}Header,", operation.response_name, code.to_uppercase()));
                }
                state.push_path(")),\n");

                state.push_async_path(format!("                None => Ok({}::NoContent{}(", operation.response_name.as_str(), code));
                if elem["headers"].is_object() {
                    state.push_async_path(format!(" {}{}Header,", operation.response_name, code.to_uppercase()));
                }
                state.push_async_path(")),\n");
            }

            state.push_path("                _=> Err(ApiError::UnexpectedContentTypeBlocking(request_url, request_headers, response))\n");
            state.push_path("            },\n");

            state.push_async_path("                _=> Err(ApiError::UnexpectedContentTypeAsync(request_url, request_headers, response))\n");
            state.push_async_path("            },\n");
        }

        if !responses["default"].is_object() {
            state.push_path("            _ => Err(ApiError::UnexpectedStatusCodeBlocking(request_url, request_headers, response))\n");
            state.push_async_path("            _ => Err(ApiError::UnexpectedStatusCodeAsync(request_url, request_headers, response))\n");
        }
    } else {
        state.push_path("            _ => Err(ApiError::UnexpectedStatusCodeBlocking(request_url, request_headers, response))\n");
        state.push_async_path("            _ => Err(ApiError::UnexpectedStatusCodeAsync(request_url, request_headers, response))\n");
    }

    state.push_path("        }\n");
    state.push_path("    }\n");
    state.push_async_path("        }\n");
    state.push_async_path("    }\n");


}

fn generate_ffi_operation_response_body_getter<A: ToString, B: ToString, C: ToString>(state: &mut State, response_name: A, enum_constant_name: B, body_name: C, arg_count: u32) {
    let response_name = response_name.to_string();
    let enum_constant_name = enum_constant_name.to_string();
    let body_name = body_name.to_string();
    if body_name == "OString" {
        //Handle text/plain case
        state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_{}_body(inst: *const {}, buffer: *mut c_char, len: *mut usize) -> bool {{\n", state.ffi_prefix, state.ffi_accessor_prefix, response_name, enum_constant_name, response_name));
        state.push_ffi("    if len.is_null() {\n");
        state.push_ffi("        ffi_abort(\"len null pointer\");\n");
        state.push_ffi("        unreachable!()\n");
        state.push_ffi("    }\n");
        state.push_ffi("    match inst.as_ref() {\n");
        state.push_ffi("        None => {\n");
        state.push_ffi("            ffi_abort(\"inst null pointer\");\n");
        state.push_ffi("            unreachable!()\n");
        state.push_ffi("        }\n");
        state.push_ffi("        Some(en) => match en {\n");
        match arg_count {
            1 => state.push_ffi(format!("            {}::{}(body) => {{\n", response_name, enum_constant_name)),
            2 => state.push_ffi(format!("            {}::{}(body, _) => {{\n", response_name, enum_constant_name)),
            _ => panic!("not implemented"),
        }
        state.push_ffi("                if body.is_none() {\n");
        state.push_ffi("                    len.write_unaligned(0);\n");
        state.push_ffi("                    return true;\n");
        state.push_ffi("                }\n");

        state.push_ffi("                let bytes = body.as_ref().unwrap().as_bytes();\n");
        state.push_ffi("                if len.read_unaligned() < bytes.len()+1 {\n");
        state.push_ffi("                    len.write_unaligned(bytes.len()+1);\n");
        state.push_ffi("                    return false;\n");
        state.push_ffi("                }\n");
        state.push_ffi("                len.write_unaligned(bytes.len()+1);\n");
        state.push_ffi("                if buffer.is_null() {\n");
        state.push_ffi("                    return true;\n");
        state.push_ffi("                }\n");
        state.push_ffi("                for (idx, ele) in bytes.iter().enumerate() {\n");
        state.push_ffi("                    match *ele as c_char {\n");
        state.push_ffi("                        0 => buffer.wrapping_add(idx).write_unaligned(32),\n");
        state.push_ffi("                        e => buffer.wrapping_add(idx).write_unaligned(e)\n");
        state.push_ffi("                    }\n");
        state.push_ffi("                }\n");
        state.push_ffi("                buffer.wrapping_add(bytes.len()).write_unaligned(0);\n");
        state.push_ffi("                return true;\n");
        state.push_ffi("            },\n");
        state.push_ffi("            _=> {\n");
        state.push_ffi("                ffi_abort(\"wrong type\");\n");
        state.push_ffi("                unreachable!()\n");
        state.push_ffi("            }\n");
        state.push_ffi("        }\n");
        state.push_ffi("    }\n");
        state.push_ffi("}\n");
        return;
    }

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_{}_body(inst: *const {}) -> *mut {} {{\n", state.ffi_prefix, state.ffi_accessor_prefix, response_name, enum_constant_name, response_name, body_name));
    state.push_ffi("    match inst.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(en) => match en {\n");
    match arg_count {
        1 => state.push_ffi(format!("            {}::{}(body) => Box::into_raw(Box::new(body.clone())),\n", response_name, enum_constant_name)),
        2 => state.push_ffi(format!("            {}::{}(body, _) => Box::into_raw(Box::new(body.clone())),\n", response_name, enum_constant_name)),
        _ => panic!("not implemented"),
    }
    state.push_ffi("            _=> {\n");
    state.push_ffi("                ffi_abort(\"wrong type\");\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");
}

fn generate_ffi_operation_response_header_getter<A: ToString, B: ToString, C: ToString>(state: &mut State, response_name: A, enum_constant_name: B, header_name: C, arg_count: u32) {
    let response_name = response_name.to_string();
    let enum_constant_name = enum_constant_name.to_string();
    let body_name = header_name.to_string();

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_{}_header(inst: *const {}) -> *mut {} {{\n", state.ffi_prefix, state.ffi_accessor_prefix, response_name, enum_constant_name, response_name, body_name));
    state.push_ffi("    match inst.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi("            ffi_abort(\"inst null pointer\");\n");
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(en) => match en {\n");
    match arg_count {
        2 => state.push_ffi(format!("            {}::{}(_, header) => Box::into_raw(Box::new(header.clone())),\n", response_name, enum_constant_name)),
        _ => panic!("not implemented"),
    }
    state.push_ffi("            _=> {\n");
    state.push_ffi("                ffi_abort(\"wrong type\");\n");
    state.push_ffi("                unreachable!()\n");
    state.push_ffi("            }\n");
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");
}

fn generate_operation_response_enum(state: &mut State, operation: &Operation, responses: &JsonValue) {
    state.push(format!("\npub enum {} {{\n", operation.response_name));
    generate_ffi_free(state, &operation.response_name);
    let mut response_enum_constants = Vec::new();
    let mut enum_object_names = HashMap::new();
    let mut enum_hdr_object_names = HashMap::new();
    if responses.is_object() {
        for (code, elem) in responses.entries() {
            if !elem.is_object() {
                continue
            }

            let mut content = false;

            for (content_type, relem) in elem["content"].entries() {
                content = true;
                let enum_name = escape_content_type_for_result_enum(code, content_type);
                let enum_param_name;

                match content_type {
                    "application/json" => {
                        let struct_name = relem["schema"]["$ref"].as_str();
                        if struct_name.is_none() {
                            match relem["schema"]["type"].as_str() {
                                Some("string") => todo!(),
                                Some("integer") => todo!(),
                                _ => todo!(),
                            }
                        }
                        let struct_name = struct_name.unwrap();
                        let struct_name = get_name_from_ref(struct_name);
                        let struct_name = state.struct_name_map.get(struct_name.as_str()).unwrap().clone();
                        enum_param_name = struct_name.clone();
                        let ff = format!("    {}({},", enum_name, struct_name);
                        state.push(ff);
                    }
                    "text/plain" => {
                        enum_param_name = "OString".to_string();
                        let ff = format!("    {}(OString,", enum_name);
                        state.push(ff);
                    }
                    _ => {
                        enum_param_name = "Stream".to_string();
                        state.push(format!("    {}(Stream,", enum_name.as_str()));
                    }
                }

                enum_object_names.insert(enum_name.clone(), enum_param_name.clone());

                if elem["headers"].is_object() {
                    let hdr_name = format!("{}{}Header", operation.response_name, code.to_uppercase());
                    generate_ffi_operation_response_body_getter(state, &operation.response_name, &enum_name, &enum_param_name, 2);
                    generate_ffi_operation_response_header_getter(state, &operation.response_name, &enum_name, &hdr_name, 2);
                    state.push(format!(" {},", hdr_name));
                    response_enum_constants.push((enum_name.clone(), 2));
                    enum_hdr_object_names.insert(enum_name.clone(), hdr_name.clone());
                } else {
                    generate_ffi_operation_response_body_getter(state, &operation.response_name, &enum_name, &enum_param_name, 1);
                    response_enum_constants.push((enum_name, 1));
                }
                state.push("),\n")
            }

            if !content {
                let enum_name = format!("NoContent{}", code);
                state.push("    ");
                state.push(enum_name.as_str());
                state.push("(");
                if elem["headers"].is_object() {
                    let hdr_name = format!("{}{}Header", operation.response_name, code.to_uppercase());
                    state.push(format!(" {},", hdr_name.clone()));
                    response_enum_constants.push((enum_name.clone(), 1));
                    enum_hdr_object_names.insert(enum_name.clone(), hdr_name.clone());
                } else {
                    response_enum_constants.push((enum_name, 0));
                }
                state.push("),\n")
            }
        }
    }

    response_enum_constants.push(("Custom".to_string(), 3));
    state.push("    Custom(Box<dyn Any+Send>, StatusCode, HeaderMap)\n");
    state.push("}\n");

    state.push(format!("\nimpl {} {{\n", operation.response_name));
    for (name, count) in &response_enum_constants {
        let result_tuple = match count {
            0 => "()".to_string(),
            1 => enum_object_names.get(name).unwrap().clone(),
            2 => format!("({}, {})", enum_object_names.get(name).unwrap(), enum_hdr_object_names.get(name).unwrap()),
            3 => continue,
            _=> panic!("Not implemented yet {}", count)
        };

        state.push(format!("    pub fn is_{}(&self) -> bool {{\n", name.to_snake_case()));
        state.push("        match self {\n");        match count {
            0 => state.push(format!("            {}::{}() => true,\n",  operation.response_name, name)),
            1 => state.push(format!("            {}::{}(_) => true,\n",  operation.response_name, name)),
            2 => state.push(format!("            {}::{}(_, _) => true,\n",  operation.response_name, name)),
            _=> panic!("Not implemented yet {}", count)
        }
        state.push("            _=> false\n");
        state.push("        }\n");
        state.push("    }\n");

        state.push(format!("    pub fn try_into_{}(self) -> Option<{}> {{\n", name.to_snake_case(), result_tuple));
        state.push("        match self {\n");        match count {
            0 => state.push(format!("            {}::{}() => Some(()),\n",  operation.response_name, name)),
            1 => state.push(format!("            {}::{}(a) => Some(a),\n",  operation.response_name, name)),
            2 => state.push(format!("            {}::{}(a, b) => Some((a, b)),\n",  operation.response_name, name)),
            _=> panic!("Not implemented yet {}", count)
        }
        state.push("            _=> None\n");
        state.push("        }\n");
        state.push("    }\n");

        state.push(format!("    pub fn try_clone_as_{}(&self) -> Option<{}> {{\n", name.to_snake_case(), result_tuple));
        state.push("        match self {\n");        match count {
            0 => state.push(format!("            {}::{}() => Some(()),\n",  operation.response_name, name)),
            1 => state.push(format!("            {}::{}(a) => Some(a.clone()),\n",  operation.response_name, name)),
            2 => state.push(format!("            {}::{}(a, b) => Some((a.clone(), b.clone())),\n",  operation.response_name, name)),
            _=> panic!("Not implemented yet {}", count)
        }
        state.push("            _=> None\n");
        state.push("        }\n");
        state.push("    }\n");

        //state.push(format!("pub fn into_{}(Self) -> Option<{}>", name));
    }
    state.push("}\n");

    state.insert_ffi();


    //FFI Enum wrapper
    state.push_ffi("#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n");
    state.push_ffi("#[repr(C)]\n");
    state.push_ffi("#[derive(Debug, Clone, PartialEq, Eq, Hash)]\n");
    state.push_ffi(format!("pub(crate) enum {}Type {{\n", operation.response_name));
    for (name, _count) in &response_enum_constants {
        state.push_ffi(format!("    {}{},\n", operation.response_name, name));
    }
    state.push_ffi("}\n");

    state.push_ffi(format!("\n#[cfg(all(feature = \"ffi\", feature = \"blocking\"))]\n#[cfg(not(target_arch = \"wasm32\"))]\n#[no_mangle] pub(crate) unsafe extern \"C\" fn {}{}{}_type(inst: *const {}) -> {}Type {{\n", state.ffi_prefix, state.ffi_accessor_prefix, operation.response_name, operation.response_name, operation.response_name));
    state.push_ffi("    match inst.as_ref() {\n");
    state.push_ffi("        None => {\n");
    state.push_ffi(format!("            ffi_abort(\"{}{}{}_type was called with a inst null pointer\");\n", state.ffi_prefix, state.ffi_accessor_prefix, operation.response_name));
    state.push_ffi("            unreachable!()\n");
    state.push_ffi("        }\n");
    state.push_ffi("        Some(en) => match en {\n");
    for (name, count) in &response_enum_constants {
        match count {
            0 => state.push_ffi(format!("            {}::{}() => {}Type::{}{},\n",  operation.response_name, name, operation.response_name, operation.response_name, name)),
            1 => state.push_ffi(format!("            {}::{}(_) => {}Type::{}{},\n",  operation.response_name, name, operation.response_name, operation.response_name, name)),
            2 => state.push_ffi(format!("            {}::{}(_, _) => {}Type::{}{},\n",  operation.response_name, name, operation.response_name, operation.response_name, name)),
            3 => state.push_ffi(format!("            {}::{}(_, _, _) => {}Type::{}{},\n",  operation.response_name, name, operation.response_name, operation.response_name, name)),
            _=> panic!("Not implemented yet {}", count)
        }
    }
    state.push_ffi("        }\n");
    state.push_ffi("    }\n");
    state.push_ffi("}\n");

    state.insert_ffi();
}

fn generate_operation_response_header(state: &mut State, operation: &Operation, responses: &JsonValue) {
    for (code, elem) in responses.entries() {
        if !elem.is_object() {
            continue
        }

        let headers = &elem["headers"];
        if !headers.is_object() {
            continue;
        }


        let field_name_map = escape_field_names(headers.entries().map(|(a, _)| a.to_string()).collect());

        let hdr_name = format!("{}{}Header", operation.response_name, code.to_uppercase());
        generate_ffi_free_new(state, &hdr_name);

        state.push("\n#[derive(Debug, Clone, Default)]\n");
        state.push(format!("pub struct {} {{\n", &hdr_name));
        for (name, _elem) in headers.entries() {
            let field_name = field_name_map.get(name).unwrap();
            state.push(format!("    pub {}: OString,\n", field_name));
            let ffi_fn_name = format!("struct_{}_field_{}", hdr_name.to_snake_case(), field_name.to_snake_case());
            generate_string_ffi_getter_setter(state, &hdr_name, field_name, ffi_fn_name);
        }
        state.push("}\n");
        state.push(format!("\nimpl From<&HeaderMap> for {} {{\n", hdr_name));
        state.push("    fn from(value: &HeaderMap) -> Self {\n");
        state.push("        Self {\n");
        for (name, _elem) in headers.entries() {
            let field_name = field_name_map.get(name).unwrap();
            state.push(format!("            {}: value.get(\"{}\").map(|h| h.to_str().ok()).filter(|o| o.is_some()).map(|o| o.unwrap().to_string()).into(),\n", field_name, name));
        }
        state.push("        }\n");
        state.push("    }\n");
        state.push("}\n");
        state.push(format!("\nimpl From<HeaderMap> for {} {{\n", hdr_name));
        state.push("    fn from(value: HeaderMap) -> Self {\n");
        state.push("        Self {\n");
        for (name, _elem) in headers.entries() {
            let field_name = field_name_map.get(name).unwrap();
            state.push(format!("            {}: value.get(\"{}\").map(|h| h.to_str().ok()).filter(|o| o.is_some()).map(|o| o.unwrap().to_string()).into(),\n", field_name, name));
        }
        state.push("        }\n");
        state.push("    }\n");
        state.push("}\n");
        state.insert_ffi();
    }
}

fn generate_paths(state: &mut State, _schema: &JsonValue) {
    for op in &state.operations.clone() {
        generate_operation(state, op);
    }

    generate_ffi_free(state, "ApiClient");
    state.insert_ffi();

    state.push("\n#[derive(Debug)]\n");
    state.push("pub struct ApiClient {\n");
    state.push("    #[cfg(feature = \"blocking\")]\n#[cfg(not(target_arch = \"wasm32\"))]\n");
    state.push("    pub client_blocking: reqwest::blocking::Client,\n");
    state.push("    #[cfg(any(feature = \"async\", target_arch = \"wasm32\"))]\n");
    state.push("    pub client_async: reqwest::Client,\n");
    state.push("    pub base_url: String,\n");
    state.push("    request_customizer: Box<dyn RequestCustomizer>,\n");
    state.push("    response_customizer: Box<dyn ResponseCustomizer>,\n");
    state.push("}\n");

    state.push("\nimpl Clone for ApiClient {\n");
    state.push("    fn clone(&self) -> Self {\n");
    state.push("        Self {\n");
    state.push("            #[cfg(feature = \"blocking\")]\n#[cfg(not(target_arch = \"wasm32\"))]\n");
    state.push("            client_blocking: self.client_blocking.clone(),\n");
    state.push("            #[cfg(any(feature = \"async\", target_arch = \"wasm32\"))]\n");
    state.push("            client_async: self.client_async.clone(),\n");
    state.push("            base_url: self.base_url.clone(),\n");
    state.push("            request_customizer: self.request_customizer.clone_to_box(),\n");
    state.push("            response_customizer: self.response_customizer.clone_to_box()\n");
    state.push("        }\n");
    state.push("    }\n");
    state.push("}\n");

    state.push("\nimpl ApiClient {\n");
    state.push("\n    #[cfg(feature = \"blocking\")]\n");
    state.push("     #[cfg(not(feature = \"async\"))]\n");
    state.push("     #[cfg(not(target_arch = \"wasm32\"))]\n");
    state.push("    pub fn new(client_blocking: reqwest::blocking::Client, base_url: &str) -> ApiClient {\n");
    state.push("        ApiClient {\n");
    state.push("            client_blocking: client_blocking,\n");
    state.push("            base_url: base_url.to_string(),\n");
    state.push("            request_customizer: Box::new(DefaultCustomizer::default()),\n");
    state.push("            response_customizer: Box::new(DefaultCustomizer::default()),\n");
    state.push("        }\n");
    state.push("    }\n");
    state.push("\n    #[cfg(any(feature = \"async\", target_arch = \"wasm32\"))]\n");
    state.push("     #[cfg(not(feature = \"blocking\"))]\n");
    state.push("    pub fn new(client_async: reqwest::Client, base_url: &str) -> ApiClient {\n");
    state.push("        ApiClient {\n");
    state.push("            client_async: client_async,\n");
    state.push("            base_url: base_url.to_string(),\n");
    state.push("            request_customizer: Box::new(DefaultCustomizer::default()),\n");
    state.push("            response_customizer: Box::new(DefaultCustomizer::default()),\n");
    state.push("        }\n");
    state.push("    }\n");
    state.push("\n    #[cfg(feature = \"async\")]\n");
    state.push("     #[cfg(not(target_arch = \"wasm32\"))]\n");
    state.push("     #[cfg(feature = \"blocking\")]\n");
    state.push("    pub fn new(client_blocking: reqwest::blocking::Client, client_async: reqwest::Client, base_url: &str) -> ApiClient {\n");
    state.push("        ApiClient {\n");
    state.push("            client_blocking: client_blocking,\n");
    state.push("            client_async: client_async,\n");
    state.push("            base_url: base_url.to_string(),\n");
    state.push("            request_customizer: Box::new(DefaultCustomizer::default()),\n");
    state.push("            response_customizer: Box::new(DefaultCustomizer::default()),\n");
    state.push("        }\n");
    state.push("    }\n");
    state.push("\n    pub fn get_request_customizer<T: RequestCustomizer + 'static>(&self) -> Option<&T> {\n");
    state.push("        self.request_customizer.as_any_ref().downcast_ref()\n");
    state.push("    }\n");
    state.push("\n    pub fn get_request_customizer_mut<T: RequestCustomizer + 'static>(&mut self) -> Option<&mut T> {\n");
    state.push("        self.request_customizer.as_any_ref_mut().downcast_mut()\n");
    state.push("    }\n");
    state.push("\n    pub fn get_response_customizer<T: ResponseCustomizer + 'static>(&self) -> Option<&T> {\n");
    state.push("        self.response_customizer.as_any_ref().downcast_ref()\n");
    state.push("    }\n");
    state.push("\n    pub fn get_response_customizer_mut<T: ResponseCustomizer + 'static>(&mut self) -> Option<&mut T> {\n");
    state.push("        self.response_customizer.as_any_ref_mut().downcast_mut()\n");
    state.push("    }\n");
    state.push("\n    pub fn replace_request_customizer(&mut self, mut customizer:  Box<dyn RequestCustomizer>) -> Box<dyn RequestCustomizer> {\n");
    state.push("        std::mem::swap(&mut self.request_customizer, &mut customizer);\n");
    state.push("        customizer\n");
    state.push("    }\n");
    state.push("\n    pub fn replace_response_customizer(&mut self, mut customizer:  Box<dyn ResponseCustomizer>) -> Box<dyn ResponseCustomizer> {\n");
    state.push("        std::mem::swap(&mut self.response_customizer, &mut customizer);\n");
    state.push("        customizer\n");
    state.push("    }\n");
    state.insert_path();
    state.insert_async_path();
    state.push("}\n");
}