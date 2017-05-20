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

use quote::{
    Ident,
    Tokens,
};

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

        //header.write_all(generate_class_header(&used_classes, &class).as_bytes());

        //let mut implementation =
        //    File::create((base_dir.to_string() + "gen/" + strip_name(&class.name) + ".rs")
        //                     .as_str())
        //            .unwrap();
        generate_class_implementation(&mut icalls, &used_classes, &class);
        //                             .as_bytes());
    }

    //let mut icall_implmentation = File::create((base_dir.to_string() + "gen/__icalls.rs").as_str())
    //    .unwrap();

    println!("{}", generate_icall_implementation(&json, &icalls).as_str());
    //icall_implmentation.write_all(generate_icall_implementation(&json, &icalls).as_bytes());
}

fn get_used_classes(class: &GodotClass) -> HashSet<&String> {

	let mut classes = HashSet::new();

	// classes.insert(&class.base_class);

	for method in &class.methods {
		if !is_primitive(&method.return_type) &&!is_core_type(&method.return_type) && !classes.contains(&method.return_type) {
			classes.insert(&method.return_type);
		}

		for argument in &method.arguments {
			if !is_primitive(&argument._type) &&!is_core_type(&argument._type) && !classes.contains(&argument._type) {
				classes.insert(&argument._type);
			}
		}
	}

	return classes;
}

fn generate_class_implementation(icalls: &mut HashSet<(String, Vec<String>)>, used_classes: &HashSet<&String>, class: &GodotClass) -> String {
        let mut contents = String::new();

	contents = contents + "#include \"" + strip_name(&class.name) + ".hpp" + "\"\n";

	contents = contents + "\n#include \"core/CoreTypes.hpp\"\n";

	contents = contents + "\n#include \"Godot.hpp\"\n\n";

	if class.instanciable {
		contents = contents + "#include \"ClassDB.hpp\"\n";
	}

	contents = contents + "\n#include \"__icalls.hpp\"\n\n\n";

	for used_class in used_classes {
		contents = contents + "#include \"" + strip_name(used_class) + ".hpp\"\n";
	}

	contents = contents + "\n\n";

	contents = contents + "namespace godot {\n\n";

	let core_obj_name = {
		let mut name = String::new();
		if class.singleton {
			name = name + "___static_object_" + strip_name(&class.name);
		} else {
			name = name + "this";
		};
		name
	};

	contents = contents + "\n\n\n";

	if class.singleton {
		contents = contents + "static godot_object *" + core_obj_name.as_str() + ";\n\n\n\n\n";
	}


	if class.singleton {
		contents = contents + "void " + strip_name(&class.name) + "::___singleton_init()\n{\n\t"
			+ core_obj_name.as_str() + " = godot_global_get_singleton(\"" + strip_name(&class.name) + "\");\n}\n\n";
	}


	// default constructor

	{
		contents = contents + strip_name(&class.name) + " *" + strip_name(&class.name) + "::_new()\n{\n";
		contents = contents + "\tgodot_class_constructor constructor = godot_get_class_constructor(\"" + class.name.as_str() + "\");\n";
		contents = contents + "\tif (!constructor) { return nullptr; }\n";
		contents = contents + "\treturn (" + strip_name(&class.name) + " *) constructor();\n";
		contents = contents + "}\n\n";
	}


	// pointer constructor
	// {
	// 	contents = contents + "" + strip_name(&class.name) + "::" + strip_name(&class.name) + "(godot_object *ptr)\n{\n";
	// 	contents = contents + "\t__core_object = ptr;\n";
	// 	contents = contents + "}\n\n\n";
	// }

	// Object constructor
	// if !class.singleton {
	// 	contents = contents + "" + strip_name(&class.name) + "::" + strip_name(&class.name) + "(const Object *ptr)\n{\n";
	// 	contents = contents + "\t__core_object = ?;\n";
	// 	contents = contents + "}\n\n\n";
	//
	// 	contents = contents + "" + strip_name(&class.name) + "::" + strip_name(&class.name) + "(const Variant& obj)\n{\n";
	// 	contents = contents + "\t__core_object = ((Object) obj).__core_object;\n";
	// 	contents = contents + "}\n\n\n";
	// }

	if class.name != "Object" {
		contents = contents + "void " + strip_name(&class.name) + "::" + "_init()\n{\n";
		contents = contents + "\t\n";
		contents = contents + "}\n\n";
	}


	contents += "\n\n";

	for method in &class.methods {
		contents = contents + strip_name(&method.return_type) + (if !is_core_type(&method.return_type) && !is_primitive(&method.return_type) { " *" } else { " " }) + strip_name(&class.name) + "::" + escape_rust(&method.name) + "(";

		for (i, argument) in (&method.arguments).iter().enumerate() {
			if !is_primitive(&argument._type) && !is_core_type(&argument._type) {
				contents = contents + "const " + argument._type.as_str() + " *";
			} else {
				contents = contents + "const " + argument._type.as_str() + " ";
			}

			contents = contents + escape_rust(&argument.name);
			if i != method.arguments.len() - 1 {
				contents += ", ";
			}
		}

		if method.has_varargs {
			if method.arguments.len() > 0 {
				contents += ", ";
			}
			contents = contents + "const Array& __var_args";
		}

		contents = contents + ")" + if method.is_const && !class.singleton { " const" } else { "" } + "\n{\n";


		if class.singleton {
			contents = contents + "\tif (" + core_obj_name.as_str() + " == 0) {\n";
			contents = contents + "\t\t___singleton_init();\n";
			contents = contents + "\t}\n\n";
		}

		if method.is_virtual || method.has_varargs {

			contents = contents + "\tArray __args;\n";

			// fill in the args
			for arg in &method.arguments {
				contents = contents + "\t__args.append(" + escape_rust(&arg.name) + ");\n";
			}

			if method.has_varargs {
				contents = contents + "\tfor (int i = 0; i < __var_args.size(); i++) {\n";
				contents = contents + "\t\t__args.append(__var_args[i]);\n";
				contents = contents + "\t}\n";
			}

			contents = contents + "\t";

			if method.return_type != "void" {
				contents = contents + "return ";

				if !is_primitive(&method.return_type) && !is_core_type(&method.return_type) {
					contents = contents + "(" + strip_name(&method.return_type) + " *) (Object *) ";
				}
			}

			contents = contents + "((Object *) " + core_obj_name.as_str() + ")->callv(\"" + method.name.as_str() + "\", __args);\n";
		} else {
			contents = contents + "\tstatic godot_method_bind *mb = NULL;\n"
				+ "\tif (mb == NULL) {\n"
				+ "\t\tmb = godot_method_bind_get_method(\"" + class.name.as_str() + "\", \"" + method.name.as_str() + "\");\n"
				+ "\t}\n\t";


			if method.return_type != "void" {
				contents = contents + "return ";
				if !is_primitive(&method.return_type) && !is_core_type(&method.return_type) {
					contents = contents + "(" + strip_name(&method.return_type) + " *) ";
				}
			}

			let mut args = Vec::new();

			fn get_icall_type_name(t: &String) -> String {
				if is_core_type(t) || is_primitive(t) {
					t.clone()
				} else {
					String::from("Object")
				}
			}

			for arg in &method.arguments {
				args.push(get_icall_type_name(&arg._type));
			}

			let icallsig = (get_icall_type_name(&method.return_type), args);

			let name = get_icall_name(&icallsig);

			icalls.insert(icallsig);


			contents = contents + name.as_str() + "(mb, (godot_object *) " + core_obj_name.as_str();

			for arg in &method.arguments {
				contents = contents + ", " + escape_rust(&arg.name);
			}

			// if !is_primitive(&method.return_type) && !is_core_type(&method.return_type) {
			// 	contents = contents + ")";
			// }
			contents = contents + ");\n";
		}

		contents = contents + "}\n\n";
	}

	// if class.instanciable {

	// 	contents = contents + strip_name(&class.name) + " " + strip_name(&class.name) + "::__new() {\n";
	// 	contents = contents + "\tObject ptr = ClassDB::instance(\"" + class.name.as_str() + "\");\n";
	// 	contents = contents + "\treturn ptr;\n";
	// 	contents = contents + "}\n\n";

	// 	contents = contents + "void " + strip_name(&class.name) + "::__destroy() {\n";
	// 	contents = contents + "\tgodot_object_destroy(__core_object);\n";
	// 	contents = contents + "}\n\n\n";

		/* if class.base_class == "" {
			// Object
			contents = contents + "Variant::operator Object()const {\n\n";
			contents = contents + "\treturn Object(godot_variant_as_object(&_godot_variant));\n\n";
			contents = contents + "}\n\n";
		} */
	// }

	contents = contents + "}\n";

	contents
}

fn get_icall_name(sig: &(String, Vec<String>)) -> String {

	let &(ref ret, ref args) = sig;

	let mut name = String::new();

	name = name + "___godot_icall_";

	name = name + strip_name(&ret);

	for arg in args {
		name = name + "_" + strip_name(&arg);
	}

	name
}
fn get_icall_name_ref(sig: (&String, &Vec<String>)) -> String {

	let (ref ret, args) = sig;

	let mut name = String::new();

	name = name + "___godot_icall_";

	name = name + strip_name(&ret);

	for arg in args {
		name = name + "_" + strip_name(&arg);
	}

	name
}

fn generate_icall_implementation(class_api: &Vec<GodotClass>,
                                 icalls: &HashSet<(String, Vec<String>)>)
                                 -> Tokens {

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
            format!("gdnative_sys::core_types::{}", t)
        } else {
            format!("*mut {}", t)
        }
    }

    let is_reference = |name: &String| class_api.iter()
        .find(|class| &class.name == name)
        .and_then(|class| Some(class.is_reference))
        .unwrap_or(false);

    let mut contents = Tokens::new();
    
    contents.append(quote!{
        use gdnative_sys;
    });

    for &(ref ret, ref args) in icalls {
        let icall_name_ref = Ident::from(get_icall_name_ref((ret, args)));
        let icall_ret_type = Ident::from(return_type(ret));

        let func_args: Vec<Tokens> = args.iter()
            .zip(0..)
            .map(|(arg, i)| {
                let arg_name = Ident::from(format!("arg{}", i));
                let arg_type = Ident::from(return_type(arg));
                if is_primitive(&arg) {
                    quote!{ #arg_name: #arg_type }
                }
                else {
                    quote!{ #arg_name: *const #arg_type }
                }
            })
            .collect();

        contents.append(quote!{
            pub unsafe fn #icall_name_ref(mb: *mut gdnative_sys::godot_method_bind, inst: *mut gdnative_sys::godot_object, #(#func_args),*) -> #icall_ret_type
        });

        contents.append({
            // Generate the function body
            let mut icall_body = Tokens::new();

            if ret != "void" {
                let stripped_ret = Ident::from(strip_name(ret));
                icall_body.append(quote!{ let ret: #icall_ret_type = std::mem::uninitialized(); });
            }

            let arg_list: Vec<Tokens> = args.iter()
                .zip(0..)
                .map(|(arg, i)| {
                    let arg_name = Ident::new(format!("arg{}", i));
                    if is_primitive(arg) || is_core_type(arg) {
                        quote!{ std::mem::transmute::<*const std::os::raw::c_void>(&#arg_name as *const _) }
                    } else {
                        quote!{ std::mem::transmute::<*const std::os::raw::c_void>(#arg_name) }
                    }
                })
                .collect();

            icall_body.append(quote!{ let args: *mut *const std::os::raw::c_void = &[ #(#arg_list),* ].as_mut_ptr(); });

            let ret_ptr = if ret != "void" { quote!{ &ret as *mut _ } } else { quote!{ std::ptr::null_mut() } };
            icall_body.append(quote!{ gdnative_sys::godot_method_bind_ptrcall(mb, inst, args, #ret_ptr); });

            if ret != "void" {
                icall_body.append(quote!{ ret });
            }

            quote!{ {#icall_body} }
        });
    }

    contents
}

fn strip_name(s: &String) -> &str {
    if s.starts_with("_") {
        &s[1..]
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
