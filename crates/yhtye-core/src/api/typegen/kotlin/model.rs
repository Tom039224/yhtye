//! The API types as the Kotlin generator sees them, read from the JSON Schemas
//! that schemars derives from the Rust types (so serde's attributes are
//! interpreted by schemars, not here).
//!
//! Two schemas are read side by side: the serialize contract (which fields
//! Rust always writes) and the deserialize contract (which fields Rust may go
//! without). Anything the generator does not know how to map fails loudly:
//! an unknown schema keyword, an inline enum, a tuple of mixed types, ...

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

/// Schema keywords the generator understands. Any other one fails the generation.
const KNOWN_KEYWORDS: &[&str] = &[
    "type",
    "format",
    "minimum",
    "maximum",
    "description",
    "title",
    "default",
    "items",
    "prefixItems",
    "minItems",
    "maxItems",
    "$ref",
    "anyOf",
    "oneOf",
    "properties",
    "required",
    "additionalProperties",
    "const",
    "enum",
];

/// A value type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Bool,
    /// A 32-bit (or smaller) integer.
    Int,
    /// `u32`, `usize`, `u64` and `i64`: ms timestamps and sequence numbers.
    Long,
    Double,
    String,
    /// `serde_json::Value` (ACP payloads, tool arguments).
    Json,
    List(Box<Ty>),
    Map(Box<Ty>),
    Nullable(Box<Ty>),
    /// A top-level definition.
    Named(String),
    /// An inline object, declared inside its owner: the path from the
    /// top-level definition (`["AgentEvent", "Exited", "Data"]`).
    Inline(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Field {
    /// The JSON key.
    pub key: String,
    pub ty: Ty,
    pub doc: Option<String>,
    /// Rust always writes it (serialize contract); otherwise it is left out
    /// when it has its default (`skip_serializing_if`).
    pub always_written: bool,
    /// Rust needs it (deserialize contract); otherwise it has a default.
    pub needed: bool,
    /// The default value Rust uses when it is missing, if schemars knows it.
    pub default: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    pub wire: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    /// Kotlin class name.
    pub name: String,
    pub doc: Option<String>,
    pub shape: Shape,
}

#[derive(Debug, Clone)]
pub enum Shape {
    /// An object with these fields (none: a unit variant).
    Fields(Vec<Field>),
    /// The whole value is another definition (an untagged newtype variant).
    Newtype(String),
}

#[derive(Debug, Clone)]
pub enum Body {
    Struct(Vec<Field>),
    /// A string enum.
    Enum(Vec<EnumValue>),
    /// An object whose `tag` key (a string) says which variant it is
    /// (serde's internally and adjacently tagged enums).
    Tagged {
        tag: String,
        /// `(tag value, variant)`.
        variants: Vec<(String, Variant)>,
    },
    /// Told apart by which keys are present, in order (serde's untagged and
    /// externally tagged enums).
    Untagged(Vec<Variant>),
}

#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub doc: Option<String>,
    pub body: Body,
    /// Inline objects declared inside it: path below the definition → fields.
    pub inline: BTreeMap<Vec<String>, Vec<Field>>,
}

/// Every definition, by Kotlin name.
pub type Model = BTreeMap<String, Def>;

/// Builds the model from the `$defs` of both contracts.
pub fn build(ser: &Map<String, Value>, de: &Map<String, Value>) -> Model {
    let mut model = Model::new();
    for (schema_name, s) in ser {
        let d = de
            .get(schema_name)
            .unwrap_or_else(|| panic!("{schema_name}: only in the serialize contract"));
        let name = kotlin_type_name(schema_name);
        let mut cx = Cx {
            path: vec![name.clone()],
            inline: BTreeMap::new(),
        };
        let body = cx.body(s, d);
        let def = Def {
            name: name.clone(),
            doc: doc(s),
            body,
            inline: cx.inline,
        };
        assert!(
            model.insert(name.clone(), def).is_none(),
            "two schemas map to the Kotlin name {name}"
        );
    }
    for d in de.keys() {
        assert!(ser.contains_key(d), "{d}: only in the deserialize contract");
    }
    model
}

/// `Result_of_string_or_AgentError` → `ResultOfStringOrAgentError`.
pub fn kotlin_type_name(schema_name: &str) -> String {
    schema_name.split('_').map(capitalize).collect()
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// `snake_case` → `PascalCase`.
pub fn pascal(s: &str) -> String {
    s.split('_').map(capitalize).collect()
}

struct Cx {
    /// Where we are, for inline names and messages.
    path: Vec<String>,
    inline: BTreeMap<Vec<String>, Vec<Field>>,
}

fn doc(s: &Value) -> Option<String> {
    s.get("description")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn obj<'a>(s: &'a Value, at: &str) -> &'a Map<String, Value> {
    let o = s
        .as_object()
        .unwrap_or_else(|| panic!("{at}: expected a schema object, got {s}"));
    if let Some(k) = o.keys().find(|k| !KNOWN_KEYWORDS.contains(&k.as_str())) {
        panic!("{at}: schema keyword `{k}` is not supported by the Kotlin generator");
    }
    o
}

fn branches(s: &Value) -> Option<&Vec<Value>> {
    s.get("oneOf")
        .or_else(|| s.get("anyOf"))
        .and_then(Value::as_array)
}

fn is_string_enum(s: &Value) -> bool {
    s.get("type") == Some(&Value::from("string"))
        && (s.get("enum").is_some() || s.get("const").is_some())
}

/// The `const` string of property `key` of an object schema.
fn const_prop<'a>(s: &'a Value, key: &str) -> Option<&'a str> {
    s.get("properties")?.get(key)?.get("const")?.as_str()
}

impl Cx {
    fn at(&self) -> String {
        self.path.join(".")
    }

    fn body(&mut self, s: &Value, d: &Value) -> Body {
        let at = self.at();
        let o = obj(s, &at);
        if is_string_enum(s) {
            return Body::Enum(enum_values(s));
        }
        if let Some(bs) = branches(s) {
            if bs.iter().all(is_string_enum) {
                return Body::Enum(bs.iter().flat_map(enum_values).collect());
            }
            let de_bs = branches(d).unwrap_or_else(|| panic!("{at}: contracts differ"));
            assert_eq!(bs.len(), de_bs.len(), "{at}: contracts differ");
            return match tag_of(bs) {
                Some(tag) => self.tagged(&tag, bs, de_bs),
                None => Body::Untagged(self.untagged(bs, de_bs)),
            };
        }
        if o.get("type") == Some(&Value::from("object")) && o.contains_key("properties") {
            return Body::Struct(self.fields(s, d, None));
        }
        panic!("{at}: unsupported definition {s}");
    }

    fn tagged(&mut self, tag: &str, bs: &[Value], de_bs: &[Value]) -> Body {
        let mut variants = Vec::new();
        for b in bs {
            let wire = const_prop(b, tag).expect("tag").to_string();
            let d = de_bs
                .iter()
                .find(|d| const_prop(d, tag) == Some(&wire))
                .unwrap_or_else(|| panic!("{}: {wire} only in one contract", self.at()));
            let name = pascal(&wire);
            self.path.push(name.clone());
            obj(b, &self.at());
            let fields = self.fields(b, d, Some(tag));
            self.path.pop();
            variants.push((
                wire,
                Variant {
                    name,
                    doc: doc(b),
                    shape: Shape::Fields(fields),
                },
            ));
        }
        Body::Tagged {
            tag: tag.to_string(),
            variants,
        }
    }

    fn untagged(&mut self, bs: &[Value], de_bs: &[Value]) -> Vec<Variant> {
        let keys: Vec<BTreeSet<String>> = bs.iter().map(required).collect();
        let mut variants = Vec::new();
        for (i, (b, d)) in bs.iter().zip(de_bs).enumerate() {
            let at = format!("{}|{i}", self.at());
            obj(b, &at);
            if let Some(r) = b.get("$ref") {
                let target = ref_name(r);
                let name = b
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| {
                        panic!("{at}: a newtype variant needs #[schemars(title = \"Name\")]")
                    })
                    .to_string();
                variants.push(Variant {
                    name,
                    doc: doc(b),
                    shape: Shape::Newtype(target),
                });
                continue;
            }
            // Named after the first key no other variant has (`Ok`, `Err`).
            let name = match b.get("title").and_then(Value::as_str) {
                Some(t) => t.to_string(),
                None => keys[i]
                    .iter()
                    .find(|k| {
                        keys.iter()
                            .enumerate()
                            .all(|(j, o)| j == i || !o.contains(*k))
                    })
                    .map(|k| pascal(k))
                    .unwrap_or_else(|| panic!("{at}: no key tells this variant apart")),
            };
            self.path.push(name.clone());
            let fields = self.fields(b, d, None);
            self.path.pop();
            variants.push(Variant {
                name,
                doc: doc(b),
                shape: Shape::Fields(fields),
            });
        }
        variants
    }

    /// The properties of an object schema, without the `skip` key (the tag).
    fn fields(&mut self, s: &Value, d: &Value, skip: Option<&str>) -> Vec<Field> {
        let at = self.at();
        assert_eq!(
            s.get("type"),
            Some(&Value::from("object")),
            "{at}: expected an object"
        );
        if let Some(extra) = s.get("additionalProperties") {
            assert_eq!(extra, &Value::Bool(false), "{at}: open object with fields");
        }
        let empty = Map::new();
        let props = s
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let de_props = d
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let written = required(s);
        let needed = required(d);
        let mut fields = Vec::new();
        for (key, p) in props {
            if Some(key.as_str()) == skip {
                continue;
            }
            assert!(de_props.contains_key(key), "{at}.{key}: contracts differ");
            self.path.push(pascal(key));
            let ty = self.ty(p, &de_props[key]);
            self.path.pop();
            fields.push(Field {
                key: key.clone(),
                ty,
                doc: doc(p),
                always_written: written.contains(key),
                needed: needed.contains(key),
                default: p.get("default").cloned(),
            });
        }
        for key in de_props.keys() {
            assert!(
                props.contains_key(key) || Some(key.as_str()) == skip,
                "{at}.{key}: contracts differ"
            );
        }
        fields
    }

    /// The type of schema `s` (serialize contract) / `d` (deserialize contract).
    fn ty(&mut self, s: &Value, d: &Value) -> Ty {
        let at = self.at();
        if s == &Value::Bool(true) {
            return Ty::Json;
        }
        let o = obj(s, &at);
        if let Some(r) = o.get("$ref") {
            return Ty::Named(kotlin_type_name(&ref_name(r)));
        }
        if let Some(bs) = o.get("anyOf").and_then(Value::as_array) {
            // `Option<Named>`: anyOf [$ref, {type: null}].
            let null = serde_json::json!({"type": "null"});
            if let [inner, n] = bs.as_slice()
                && n == &null
            {
                let de_inner = d.pointer("/anyOf/0").unwrap_or(inner);
                return Ty::Nullable(Box::new(self.ty(inner, de_inner)));
            }
            panic!("{at}: unsupported anyOf {s}");
        }
        assert!(
            !o.contains_key("oneOf") && !o.contains_key("enum") && !o.contains_key("const"),
            "{at}: inline enums are not supported; give the type a name"
        );
        let (ty, nullable) = match o.get("type") {
            Some(Value::String(t)) => (t.as_str(), false),
            Some(Value::Array(ts)) => match ts.as_slice() {
                [Value::String(t), n] if n == "null" => (t.as_str(), true),
                _ => panic!("{at}: unsupported type {s}"),
            },
            _ => panic!("{at}: no type in {s}"),
        };
        let inner = match ty {
            "boolean" => Ty::Bool,
            "string" => Ty::String,
            "number" => Ty::Double,
            "integer" => int_ty(o.get("format").and_then(Value::as_str), &at),
            "array" => self.array(o, d),
            "object" => self.object(o, s, d),
            _ => panic!("{at}: unsupported type {ty}"),
        };
        if nullable {
            Ty::Nullable(Box::new(inner))
        } else {
            inner
        }
    }

    fn array(&mut self, o: &Map<String, Value>, d: &Value) -> Ty {
        let at = self.at();
        if let Some(items) = o.get("items") {
            let de_items = d.get("items").unwrap_or(items);
            return Ty::List(Box::new(self.ty(items, de_items)));
        }
        // A tuple of one type (`(String, String)`): a fixed-length list.
        let items = o
            .get("prefixItems")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{at}: array without items"));
        let first = items.first().unwrap_or_else(|| panic!("{at}: empty tuple"));
        assert!(
            items.iter().all(|i| i == first),
            "{at}: tuples of mixed types are not supported"
        );
        Ty::List(Box::new(self.ty(first, first)))
    }

    fn object(&mut self, o: &Map<String, Value>, s: &Value, d: &Value) -> Ty {
        let at = self.at();
        if o.contains_key("properties") {
            let fields = self.fields(s, d, None);
            let path = self.path.clone();
            self.inline.insert(path[1..].to_vec(), fields);
            return Ty::Inline(path);
        }
        match o.get("additionalProperties") {
            Some(v) if v != &Value::Bool(false) => {
                let de_v = d.get("additionalProperties").unwrap_or(v);
                Ty::Map(Box::new(self.ty(v, de_v)))
            }
            _ => panic!("{at}: object without properties"),
        }
    }
}

fn int_ty(format: Option<&str>, at: &str) -> Ty {
    match format {
        Some("int8" | "int16" | "int32" | "uint8" | "uint16") => Ty::Int,
        Some("uint32" | "uint" | "int64" | "uint64" | "int") => Ty::Long,
        f => panic!("{at}: unsupported integer format {f:?}"),
    }
}

fn ref_name(r: &Value) -> String {
    r.as_str()
        .and_then(|r| r.strip_prefix("#/$defs/"))
        .unwrap_or_else(|| panic!("unsupported $ref {r}"))
        .to_string()
}

fn required(s: &Value) -> BTreeSet<String> {
    s.get("required")
        .and_then(Value::as_array)
        .map(|r| {
            r.iter()
                .map(|k| k.as_str().expect("required key").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn enum_values(s: &Value) -> Vec<EnumValue> {
    let d = doc(s);
    if let Some(c) = s.get("const") {
        return vec![EnumValue {
            wire: c.as_str().expect("string const").to_string(),
            doc: d,
        }];
    }
    let values = s.get("enum").and_then(Value::as_array).expect("enum");
    values
        .iter()
        .map(|v| EnumValue {
            wire: v.as_str().expect("string enum").to_string(),
            // A doc on a group of values belongs to none of them.
            doc: None,
        })
        .collect()
}

/// The key every branch has with a constant string, if exactly one does.
fn tag_of(bs: &[Value]) -> Option<String> {
    let mut common: Option<BTreeSet<String>> = None;
    for b in bs {
        let consts: BTreeSet<String> = b
            .get("properties")
            .and_then(Value::as_object)
            .map(|p| {
                p.iter()
                    .filter(|(_, v)| v.get("const").is_some_and(Value::is_string))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        common = Some(match common {
            None => consts,
            Some(c) => c.intersection(&consts).cloned().collect(),
        });
    }
    let common = common?;
    assert!(common.len() <= 1, "several tag candidates: {common:?}");
    common.into_iter().next()
}

/// Lookup helpers shared by the emitter and the sample walker.
pub trait ModelExt {
    fn def(&self, name: &str) -> &Def;
    /// The key sets that identify a value of `name` as an untagged variant.
    fn key_sets(&self, shape: &Shape) -> Vec<BTreeSet<String>>;
}

impl ModelExt for Model {
    fn def(&self, name: &str) -> &Def {
        self.get(name)
            .unwrap_or_else(|| panic!("unknown definition {name}"))
    }

    fn key_sets(&self, shape: &Shape) -> Vec<BTreeSet<String>> {
        match shape {
            Shape::Fields(fields) => vec![
                fields
                    .iter()
                    .filter(|f| f.always_written)
                    .map(|f| f.key.clone())
                    .collect(),
            ],
            Shape::Newtype(target) => match &self.def(target).body {
                Body::Struct(fields) => self.key_sets(&Shape::Fields(fields.clone())),
                Body::Tagged { tag, .. } => vec![BTreeSet::from([tag.clone()])],
                Body::Untagged(vs) => vs.iter().flat_map(|v| self.key_sets(&v.shape)).collect(),
                Body::Enum(_) => panic!("{target}: an enum as an untagged variant"),
            },
        }
    }
}
