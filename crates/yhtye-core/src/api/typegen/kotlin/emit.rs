//! Kotlin (kotlinx.serialization) source for the [`super::model`].
//!
//! Shapes:
//! - struct → `@Serializable data class` (camelCase properties, `@SerialName`
//!   for the JSON key); `Option<T>` → `T? = null`; a field Rust leaves out at
//!   its default (`skip_serializing_if`) → `@EncodeDefault(NEVER)`.
//! - string enum → `enum class` with a `wire` name and an `Unrecognized` entry.
//! - tagged enum → `sealed interface` + `data class` / `data object` variants,
//!   read and written by `TaggedSerializer` (handwritten, `shared.api`), with an
//!   `Unrecognized` variant for tags this client does not know.
//! - untagged enum → `sealed interface` read by which keys are present
//!   (`UntaggedSerializer`); a newtype variant is a `value class`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::Value;

use super::model::{Body, Def, Field, Model, ModelExt, Shape, Ty, pascal};

pub const PACKAGE: &str = "io.github.tom039224.yhtye.shared.api.generated";
const SUPPORT: &str = "io.github.tom039224.yhtye.shared.api";
pub const HEADER: &str = "// Generated from the Rust API types of yhtye-core \
(crates/yhtye-core/src/api/typegen/kotlin).\n// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).\n";

/// Names a generated class may not take: they would hide what the generated
/// code refers to.
const TAKEN: &[&str] = &[
    "Boolean",
    "Int",
    "Long",
    "Double",
    "String",
    "List",
    "Map",
    "JsonElement",
    "JsonObject",
    "Serializable",
    "SerialName",
    "EncodeDefault",
    "JvmInline",
];

const KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

/// The source file of one definition.
pub fn file(model: &Model, def: &Def) -> String {
    let mut e = Emitter {
        model,
        def,
        imports: BTreeSet::new(),
        reserved: reserved_names(def),
        out: String::new(),
    };
    e.def();
    let mut text = String::from(HEADER);
    if e.imports.contains("kotlinx.serialization.EncodeDefault") {
        e.imports
            .insert("kotlinx.serialization.ExperimentalSerializationApi".into());
        text.push_str("@file:OptIn(ExperimentalSerializationApi::class)\n");
    }
    let _ = writeln!(text, "\npackage {PACKAGE}\n");
    for i in &e.imports {
        let _ = writeln!(text, "import {i}");
    }
    text.push('\n');
    text.push_str(&e.out);
    text
}

/// Every class name declared inside `def` (references to top-level types with
/// these names must be qualified).
fn reserved_names(def: &Def) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = ["Unrecognized", "Serializer"]
        .into_iter()
        .map(String::from)
        .collect();
    match &def.body {
        Body::Tagged { variants, .. } => {
            names.extend(variants.iter().map(|(_, v)| v.name.clone()));
        }
        Body::Untagged(vs) => names.extend(vs.iter().map(|v| v.name.clone())),
        Body::Struct(_) | Body::Enum(_) => {}
    }
    names.extend(def.inline.keys().filter_map(|p| p.last().cloned()));
    for n in &names {
        assert!(
            !TAKEN.contains(&n.as_str()) && n != &def.name,
            "{}: a nested class may not be named {n}",
            def.name
        );
    }
    names
}

struct Emitter<'a> {
    model: &'a Model,
    def: &'a Def,
    imports: BTreeSet<String>,
    reserved: BTreeSet<String>,
    out: String,
}

/// `after_seq` → `afterSeq`, `Ok` → `ok`; keywords in backticks.
pub fn property_name(key: &str) -> String {
    let p = pascal(key);
    let mut c = p.chars();
    let name: String = c
        .next()
        .map(|f| f.to_lowercase().chain(c).collect())
        .unwrap_or_default();
    if KEYWORDS.contains(&name.as_str()) {
        format!("`{name}`")
    } else {
        name
    }
}

pub fn string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Emitter<'_> {
    fn import(&mut self, path: &str) {
        self.imports.insert(path.to_string());
    }

    fn line(&mut self, indent: usize, text: &str) {
        if text.is_empty() {
            self.out.push('\n');
        } else {
            let _ = writeln!(self.out, "{}{text}", "    ".repeat(indent));
        }
    }

    fn doc(&mut self, indent: usize, doc: Option<&str>) {
        let Some(doc) = doc else { return };
        // Kotlin comments nest: neither `*/` nor `/*` may appear inside.
        let doc = doc.replace("*/", "*&#47;").replace("/*", "/&#42;");
        self.line(indent, "/**");
        for l in doc.lines() {
            if l.is_empty() {
                self.line(indent, " *");
            } else {
                self.line(indent, &format!(" * {l}"));
            }
        }
        self.line(indent, " */");
    }

    fn def(&mut self) {
        let def = self.def;
        self.doc(0, def.doc.as_deref());
        match &def.body {
            Body::Struct(fields) => self.class(0, &def.name, &[], fields, None),
            Body::Enum(values) => self.enumeration(values),
            Body::Tagged { tag, variants } => self.tagged(tag, variants),
            Body::Untagged(variants) => self.untagged(variants),
        }
    }

    fn ty(&mut self, ty: &Ty) -> String {
        match ty {
            Ty::Bool => "Boolean".into(),
            Ty::Int => "Int".into(),
            Ty::Long => "Long".into(),
            Ty::Double => "Double".into(),
            Ty::String => "String".into(),
            Ty::Json => {
                self.import("kotlinx.serialization.json.JsonElement");
                "JsonElement".into()
            }
            Ty::List(t) => format!("List<{}>", self.ty(t)),
            Ty::Map(t) => format!("Map<String, {}>", self.ty(t)),
            Ty::Nullable(t) => format!("{}?", self.ty(t)),
            Ty::Named(n) => {
                assert!(self.model.contains_key(n), "unknown type {n}");
                if self.reserved.contains(n) {
                    format!("{PACKAGE}.{n}")
                } else {
                    n.clone()
                }
            }
            Ty::Inline(path) => path.join("."),
        }
    }

    fn default_value(&mut self, f: &Field) -> Option<String> {
        if let Ty::Nullable(_) = f.ty {
            assert!(
                matches!(f.default, None | Some(Value::Null)),
                "{}.{}: a nullable field with a non-null default",
                self.def.name,
                f.key
            );
            return Some("null".into());
        }
        if f.needed {
            assert!(
                f.always_written,
                "{}.{}: left out when writing but needed when reading",
                self.def.name, f.key
            );
            return None;
        }
        // schemars leaves the default out of a field it skips when empty
        // (`default` with `skip_serializing_if = "Vec::is_empty"`): the empty
        // list is the default then.
        let skipped_when_empty = match f.ty {
            Ty::List(_) if !f.always_written => Some(Value::Array(Vec::new())),
            Ty::Map(_) if !f.always_written => Some(Value::Object(Default::default())),
            _ => None,
        };
        let d = f
            .default
            .as_ref()
            .or(skipped_when_empty.as_ref())
            .unwrap_or_else(|| {
                panic!(
                    "{}.{}: optional field without a known default",
                    self.def.name, f.key
                )
            });
        Some(match (&f.ty, d) {
            (Ty::Bool, Value::Bool(b)) => b.to_string(),
            (Ty::Int, Value::Number(n)) => n.to_string(),
            (Ty::Long, Value::Number(n)) => format!("{n}L"),
            (Ty::Double, Value::Number(n)) => format!("{:?}", n.as_f64().expect("f64")),
            (Ty::String, Value::String(s)) => string_literal(s),
            (Ty::List(_), Value::Array(a)) if a.is_empty() => "emptyList()".into(),
            (Ty::Map(_), Value::Object(o)) if o.is_empty() => "emptyMap()".into(),
            (t, d) => panic!(
                "{}.{}: default {d} of {t:?} is not supported",
                self.def.name, f.key
            ),
        })
    }

    fn fields(&mut self, indent: usize, fields: &[Field]) {
        for f in fields {
            self.doc(indent, f.doc.as_deref());
            let name = property_name(&f.key);
            if name.trim_matches('`') != f.key {
                self.import("kotlinx.serialization.SerialName");
                self.line(indent, &format!("@SerialName({})", string_literal(&f.key)));
            }
            let default = self.default_value(f);
            if !f.always_written {
                assert!(default.is_some(), "{}.{}: no default", self.def.name, f.key);
                self.import("kotlinx.serialization.EncodeDefault");
                self.line(indent, "@EncodeDefault(EncodeDefault.Mode.NEVER)");
            }
            let ty = self.ty(&f.ty);
            let tail = default.map(|d| format!(" = {d}")).unwrap_or_default();
            self.line(indent, &format!("val {name}: {ty}{tail},"));
        }
    }

    /// A data class (or a data object without fields) at `path` below the
    /// definition, implementing `parent`.
    fn class(
        &mut self,
        indent: usize,
        name: &str,
        path: &[String],
        fields: &[Field],
        parent: Option<&str>,
    ) {
        self.import("kotlinx.serialization.Serializable");
        self.line(indent, "@Serializable");
        let supertype = parent.map(|p| format!(" : {p}")).unwrap_or_default();
        let nested: Vec<(Vec<String>, Vec<Field>)> = self
            .def
            .inline
            .iter()
            .filter(|(p, _)| p.len() == path.len() + 1 && p.starts_with(path))
            .map(|(p, f)| (p.clone(), f.clone()))
            .collect();
        if fields.is_empty() {
            assert!(nested.is_empty());
            self.line(indent, &format!("data object {name}{supertype}"));
            return;
        }
        self.line(indent, &format!("data class {name}("));
        self.fields(indent + 1, fields);
        if nested.is_empty() {
            self.line(indent, &format!("){supertype}"));
            return;
        }
        self.line(indent, &format!("){supertype} {{"));
        for (i, (p, f)) in nested.iter().enumerate() {
            if i > 0 {
                self.line(0, "");
            }
            let n = p.last().expect("name").clone();
            self.class(indent + 1, &n, p, f, None);
        }
        self.line(indent, "}");
    }

    fn sealed_header(&mut self) {
        let name = &self.def.name;
        self.import("kotlinx.serialization.Serializable");
        self.line(
            0,
            &format!("@Serializable(with = {name}.Serializer::class)"),
        );
        self.line(0, &format!("sealed interface {name} {{"));
    }

    fn tagged(&mut self, tag: &str, variants: &[(String, super::model::Variant)]) {
        let name = self.def.name.clone();
        self.sealed_header();
        for (_, v) in variants {
            let Shape::Fields(fields) = &v.shape else {
                panic!("{name}.{}: newtype variant in a tagged enum", v.name)
            };
            assert!(
                fields.iter().all(|f| f.key != tag),
                "{name}.{}: a field named like the tag `{tag}`",
                v.name
            );
            self.doc(1, v.doc.as_deref());
            self.class(
                1,
                &v.name,
                std::slice::from_ref(&v.name),
                fields,
                Some(&name),
            );
            self.line(0, "");
        }
        self.import("kotlinx.serialization.json.JsonObject");
        self.import(&format!("{SUPPORT}.UnrecognizedVariant"));
        self.import(&format!("{SUPPORT}.TaggedSerializer"));
        self.import(&format!("{SUPPORT}.TaggedVariant"));
        self.line(
            1,
            &format!(
                "/** A `{tag}` this client does not know (from a newer Yhtye). Cannot be sent. */"
            ),
        );
        self.line(1, "data class Unrecognized(");
        self.line(2, &format!("/** The unknown `{tag}`. */"));
        self.line(2, "val tag: String,");
        self.line(2, "override val raw: JsonObject,");
        self.line(1, &format!(") : {name}, UnrecognizedVariant"));
        self.line(0, "");
        self.line(1, &format!("object Serializer : TaggedSerializer<{name}>("));
        self.line(2, &format!("{},", string_literal(&name)));
        self.line(2, &format!("{},", string_literal(tag)));
        self.line(2, "listOf(");
        for (wire, v) in variants {
            self.line(
                3,
                &format!(
                    "TaggedVariant<{name}>({}, {}.serializer()) {{ it is {} }},",
                    string_literal(wire),
                    v.name,
                    v.name
                ),
            );
        }
        self.line(2, "),");
        self.line(2, "::Unrecognized,");
        self.line(1, ")");
        self.line(0, "}");
    }

    fn untagged(&mut self, variants: &[super::model::Variant]) {
        let name = self.def.name.clone();
        self.sealed_header();
        for v in variants {
            self.doc(1, v.doc.as_deref());
            match &v.shape {
                Shape::Fields(fields) => {
                    assert!(!fields.is_empty(), "{name}.{}: unit variant", v.name);
                    self.class(
                        1,
                        &v.name,
                        std::slice::from_ref(&v.name),
                        fields,
                        Some(&name),
                    );
                }
                Shape::Newtype(target) => {
                    self.import("kotlinx.serialization.Serializable");
                    self.import("kotlin.jvm.JvmInline");
                    let ty = self.ty(&Ty::Named(super::model::kotlin_type_name(target)));
                    self.line(1, "@Serializable");
                    self.line(1, "@JvmInline");
                    self.line(
                        1,
                        &format!("value class {}(val value: {ty}) : {name}", v.name),
                    );
                }
            }
            self.line(0, "");
        }
        self.import("kotlinx.serialization.json.JsonElement");
        self.import(&format!("{SUPPORT}.UnrecognizedVariant"));
        self.import(&format!("{SUPPORT}.UntaggedSerializer"));
        self.import(&format!("{SUPPORT}.UntaggedVariant"));
        self.line(
            1,
            "/** A shape this client does not know (from a newer Yhtye). Cannot be sent. */",
        );
        self.line(
            1,
            &format!("data class Unrecognized(override val raw: JsonElement) : {name}, UnrecognizedVariant"),
        );
        self.line(0, "");
        self.line(
            1,
            &format!("object Serializer : UntaggedSerializer<{name}>("),
        );
        self.line(2, &format!("{},", string_literal(&name)));
        self.line(2, "listOf(");
        for v in variants {
            let sets: Vec<String> = self
                .model
                .key_sets(&v.shape)
                .iter()
                .map(|s| {
                    let keys: Vec<String> = s.iter().map(|k| string_literal(k)).collect();
                    format!("setOf({})", keys.join(", "))
                })
                .collect();
            self.line(
                3,
                &format!(
                    "UntaggedVariant<{name}>(listOf({}), {}.serializer()) {{ it is {} }},",
                    sets.join(", "),
                    v.name,
                    v.name
                ),
            );
        }
        self.line(2, "),");
        self.line(2, "::Unrecognized,");
        self.line(1, ")");
        self.line(0, "}");
    }

    fn enumeration(&mut self, values: &[super::model::EnumValue]) {
        let name = self.def.name.clone();
        self.import("kotlinx.serialization.Serializable");
        self.import(&format!("{SUPPORT}.WireEnum"));
        self.import(&format!("{SUPPORT}.WireEnumSerializer"));
        self.line(
            0,
            &format!("@Serializable(with = {name}.Serializer::class)"),
        );
        self.line(
            0,
            &format!("enum class {name}(override val wire: String) : WireEnum {{"),
        );
        let mut seen = BTreeSet::new();
        for v in values {
            let entry = pascal(&v.wire);
            assert!(
                entry != "Unrecognized" && seen.insert(entry.clone()),
                "{name}: entry {entry} twice"
            );
            self.doc(1, v.doc.as_deref());
            self.line(1, &format!("{entry}({}),", string_literal(&v.wire)));
        }
        self.line(
            1,
            "/** A value this client does not know (from a newer Yhtye). Cannot be sent. */",
        );
        self.line(1, "Unrecognized(\"\");");
        self.line(0, "");
        self.line(
            1,
            &format!(
                "object Serializer : WireEnumSerializer<{name}>({}, {name}.entries, Unrecognized)",
                string_literal(&name)
            ),
        );
        self.line(0, "}");
    }
}
