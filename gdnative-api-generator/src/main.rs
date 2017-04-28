extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate quote;


use serde_json::*;
use std::fs::File;
use std::iter::Iterator;
use std::io::prelude::*;

use std::collections::HashSet;

use std::env;

#[derive(Deserialize)]
struct GodotClass {
    name: String,
    base_class: String,
    api_type: String,
    singleton: bool,
    instanciable: bool,
    is_reference: bool,
    constants: Map<String, Value>,
    methods: Vec<GodotMethod>,
}

#[derive(Deserialize)]
struct GodotMethod {
    name: String,
    return_type: String,
    is_editor: bool,
    is_noscript: bool,
    is_const: bool,
    is_virtual: bool,
    has_varargs: bool,
    is_from_script: bool,
    arguments: Vec<GodotArgument>,
}

#[derive(Deserialize)]
struct GodotArgument {
    name: String,
    #[serde(rename = "type")]
    _type: String,
    has_default_value: bool,
    default_value: String,
}

fn main() {
    let base_dir = match env::args().nth(2) {
        Some(x) => x,
        None => return,
    };

    let mut file = match env::args().nth(1) {
        Some(x) => File::open(x).unwrap(),
        None => return,
    };

    let mut file_contents = String::new();

    file.read_to_string(&mut file_contents);

    let json: Vec<GodotClass> = serde_json::from_str::<Vec<GodotClass>>(&file_contents).unwrap();

    let mut icalls: HashSet<(String, Vec<String>)> = HashSet::new();

    for class in &json {
        // make this toggleable with a command line switch
        // if class.api_type == "tools" {
        // 	println!("{}", class.name);
        // 	continue
        // }
        let used_classes = get_used_classes(&class);

        header.write_all(generate_class_header(&used_classes, &class).as_bytes());

        let mut implementation =
            File::create((base_dir.to_string() + "gen/" + strip_name(&class.name) + ".rs")
                             .as_str())
                    .unwrap();
        implementation.write_all(generate_class_implementation(&mut icalls, &used_classes, &class)
                                     .as_bytes());
    }

    let mut icall_implmentation = File::create((base_dir.to_string() + "gen/__icalls.rs").as_str())
        .unwrap();

    icall_implmentation.write_all(generate_icall_implementation(&json, &icalls).as_bytes());
}

fn generate_icall_implementation(class_api: &Vec<GodotClass>,
                                 icalls: &HashSet<(String, Vec<String>)>)
                                 -> String {

    fn return_type(t: &String) -> String {
        if is_primitive(t) {
            match t.as_str() {
                    "int" => "i32",
                    "bool" => "bool",
                    "real" => "f32",
                    "float" => "f32",
                    "void" => "()",
                    _ => unreachable!(),
                }
                .to_string()
        } else if is_core_type(t) {
            t.clone()
        } else {
            let s = String::new() + t.as_str() + " *";
            s
        }
    }

    let is_reference = |name: &String| class_api.iter()
        .find(|class| class.name == name)
        .and_then(|class| class.is_reference)
        .unwrap_or(false);

    let mut contents = Tokens::new();
    
    contents.append(quote!{
        use gdnative_sys;
    });

    for &(ref ret, ref args) in icalls {
        let icall_name_ref = get_icall_name_ref((ret, args));
        let icall_ret_type = return_type(ret);

        let func_args: Vec<Tokens> = args.iter()
            .zip(0..)
            .map(|(arg, i)| {
                let arg_name = Ident::from(format!("arg{}", i));
                let arg = Ident::from(arg);
                if is_primitive(&arg) {
                    quote!{ #arg_name: #arg }
                }
                else {
                    quote!{ #arg_name: *const }
                }
            })
            .collect();

        contents.append(quote!{
            pub fn #icall_name_ref(mb: *mut gdnative_sys::godot_method_bind, inst: *mut gdnative_sys::godot_object, #(#func_args),*) -> #icall_ret_type
        });

        icall_body = Tokens::new();

        if ret != "void" {
            let stripped_ret = Ident::from(strip_name(ret));
            icall_body.append(quote!{ let ret: #icall_ret_type; });
        }

        contents = contents + "\tconst void *args[" + if args.len() == 0 { "1" } else { "" } +
                   "] = {\n";

        let mut j = 0;
        for arg in args {
            contents = contents + "\t\t";
            if is_primitive(arg) {
                contents = contents + "&arg" + j.to_string().as_str();
            } else if is_core_type(arg) {
                contents = contents + "(void *) &arg" + j.to_string().as_str();
            } else {
                contents = contents + "(void *) arg" + j.to_string().as_str();
            }
            contents = contents + ",\n";
            j = j + 1;
        }

        contents = contents + "\tgodot_method_bind_ptrcall(mb, inst, args, " +
                   if ret == "void" { "NULL" } else { "&ret" } + ");\n";

        if !is_primitive(ret) && !is_core_type(ret) {
            contents = contents + "\treturn ret;\n";
        } else if ret != "void" {
            contents = contents + "\treturn ret;\n";
        }
    }

    contents
}

fn strip_name(s: &String) -> &str {
    if s.starts_with("_") {
        s[1..].as_str()
    } else {
        s.as_str()
    }
}

fn is_core_type(name: &String) -> bool {
    let core_types = vec!["Array",
                          "Basis",
                          "Color",
                          "Dictionary",
                          "Error",
                          "Image",
                          "InputEvent",
                          "NodePath",
                          "Plane",
                          "PoolByteArray",
                          "PoolIntArray",
                          "PoolRealArray",
                          "PoolStringArray",
                          "PoolVector2Array",
                          "PoolVector3Array",
                          "PoolColorArray",
                          "Quat",
                          "Rect2",
                          "Rect3",
                          "RID",
                          "String",
                          "Transform",
                          "Transform2D",
                          "Variant",
                          "Vector2",
                          "Vector3"];
    core_types.contains(&name.as_str())
}

fn is_primitive(name: &String) -> bool {
    let core_types = vec!["int", "bool", "real", "float", "void"];
    core_types.contains(&name.as_str())
}

fn escape_rust(name: &String) -> &str {
    match name.as_str() {
        "abstract" => "_abstract",
        "alignof" => "_alignof",
        "as" => "_as",
        "become" => "_become",
        "box" => "_box",
        "break" => "_break",
        "const" => "_const",
        "continue" => "_continue",
        "crate" => "_crate",
        "do" => "_do",
        "else" => "_else",
        "enum" => "_enum",
        "extern" => "_extern",
        "false" => "_false",
        "final" => "_final",
        "fn" => "_fn",
        "for" => "_for",
        "if" => "_if",
        "impl" => "_impl",
        "in" => "_in",
        "let" => "_let",
        "loop" => "_loop",
        "macro" => "_macro",
        "match" => "_match",
        "mod" => "_mod",
        "move" => "_move",
        "mut" => "_mut",
        "offsetof" => "_offsetof",
        "override" => "_override",
        "priv" => "_priv",
        "proc" => "_proc",
        "pub" => "_pub",
        "pure" => "_pure",
        "ref" => "_ref",
        "return" => "_return",
        "Self" => "_Self",
        "self" => "_self",
        "sizeof" => "_sizeof",
        "static" => "_static",
        "struct" => "_struct",
        "super" => "_super",
        "trait" => "_trait",
        "true" => "_true",
        "type" => "_type",
        "typeof" => "_typeof",
        "unsafe" => "_unsafe",
        "unsized" => "_unsized",
        "use" => "_use",
        "virtual" => "_virtual",
        "where" => "_where",
        "while" => "_while",
        "yield" => "_yield",
        x => x,
    }
}
