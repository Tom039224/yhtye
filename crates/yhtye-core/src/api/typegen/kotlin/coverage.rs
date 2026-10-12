//! Checks the contract samples against the [`super::model`]: which variants
//! they cover, and that the JSON the Rust types really write has the shape the
//! model (and so the Kotlin types) expects.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::model::{Body, Field, Model, ModelExt, Shape, Ty};

/// What the samples reached: `(definition, variant)` and definitions.
#[derive(Debug, Default)]
pub struct Seen {
    variants: BTreeSet<(String, String)>,
    defs: BTreeSet<String>,
}

/// Walks `v`, a value of definition `name`; panics where it does not fit.
pub fn walk(model: &Model, seen: &mut Seen, name: &str, v: &Value, at: &str) {
    let def = model.def(name);
    seen.defs.insert(def.name.clone());
    match &def.body {
        Body::Struct(fields) => walk_fields(model, seen, fields, as_obj(v, at), None, at),
        Body::Enum(values) => {
            let s = v
                .as_str()
                .unwrap_or_else(|| panic!("{at}: expected a string, got {v}"));
            assert!(
                values.iter().any(|e| e.wire == s),
                "{at}: `{s}` is not a value of {name}"
            );
            seen.variants.insert((def.name.clone(), s.to_string()));
        }
        Body::Tagged { tag, variants } => {
            let o = as_obj(v, at);
            let t = o
                .get(tag)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{at}: no `{tag}` in {v}"));
            let (_, variant) = variants
                .iter()
                .find(|(w, _)| w == t)
                .unwrap_or_else(|| panic!("{at}: `{t}` is not a variant of {name}"));
            seen.variants.insert((def.name.clone(), t.to_string()));
            let Shape::Fields(fields) = &variant.shape else {
                unreachable!("tagged newtype")
            };
            let at = format!("{at}.{t}");
            walk_fields(model, seen, fields, o, Some(tag), &at);
        }
        Body::Untagged(variants) => {
            let o = as_obj(v, at);
            let variant = variants
                .iter()
                .find(|var| {
                    model
                        .key_sets(&var.shape)
                        .iter()
                        .any(|keys| keys.iter().all(|k| o.contains_key(k)))
                })
                .unwrap_or_else(|| panic!("{at}: no variant of {name} matches {v}"));
            seen.variants
                .insert((def.name.clone(), variant.name.clone()));
            let at = format!("{at}.{}", variant.name);
            match &variant.shape {
                Shape::Fields(fields) => walk_fields(model, seen, fields, o, None, &at),
                Shape::Newtype(target) => walk(model, seen, target, v, &at),
            }
        }
    }
}

fn as_obj<'a>(v: &'a Value, at: &str) -> &'a Map<String, Value> {
    v.as_object()
        .unwrap_or_else(|| panic!("{at}: expected an object, got {v}"))
}

fn walk_fields(
    model: &Model,
    seen: &mut Seen,
    fields: &[Field],
    o: &Map<String, Value>,
    tag: Option<&str>,
    at: &str,
) {
    for key in o.keys() {
        assert!(
            Some(key.as_str()) == tag || fields.iter().any(|f| &f.key == key),
            "{at}: key `{key}` is not in the model"
        );
    }
    for f in fields {
        let at = format!("{at}.{}", f.key);
        match o.get(&f.key) {
            None => assert!(!f.always_written, "{at}: missing, but always written"),
            Some(v) => walk_ty(model, seen, &f.ty, v, &at),
        }
    }
}

fn walk_ty(model: &Model, seen: &mut Seen, ty: &Ty, v: &Value, at: &str) {
    let fits = match ty {
        Ty::Bool => v.is_boolean(),
        Ty::Int => v.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
        Ty::Long => v.is_i64() || v.as_u64().is_some_and(|n| i64::try_from(n).is_ok()),
        Ty::Double => v.is_number(),
        Ty::String => v.is_string(),
        Ty::Json => true,
        Ty::Nullable(inner) => {
            if !v.is_null() {
                walk_ty(model, seen, inner, v, at);
            }
            true
        }
        Ty::List(item) => {
            let items = v
                .as_array()
                .unwrap_or_else(|| panic!("{at}: expected an array"));
            for (i, x) in items.iter().enumerate() {
                walk_ty(model, seen, item, x, &format!("{at}[{i}]"));
            }
            true
        }
        Ty::Map(item) => {
            for (k, x) in as_obj(v, at) {
                walk_ty(model, seen, item, x, &format!("{at}[{k}]"));
            }
            true
        }
        Ty::Named(n) => {
            walk(model, seen, n, v, at);
            true
        }
        Ty::Inline(path) => {
            let owner = model.def(&path[0]);
            let fields = &owner.inline[&path[1..].to_vec()];
            walk_fields(model, seen, fields, as_obj(v, at), None, at);
            true
        }
    };
    assert!(fits, "{at}: {v} is not a {ty:?}");
}

/// Variants (and definitions) no sample reached.
pub fn missing(model: &Model, seen: &Seen) -> Vec<String> {
    let mut out = Vec::new();
    for def in model.values() {
        let names: Vec<String> = match &def.body {
            Body::Struct(_) => {
                if !seen.defs.contains(&def.name) {
                    out.push(def.name.clone());
                }
                continue;
            }
            Body::Enum(values) => values.iter().map(|e| e.wire.clone()).collect(),
            Body::Tagged { variants, .. } => variants.iter().map(|(w, _)| w.clone()).collect(),
            Body::Untagged(variants) => variants.iter().map(|v| v.name.clone()).collect(),
        };
        for n in names {
            if !seen.variants.contains(&(def.name.clone(), n.clone())) {
                out.push(format!("{}::{n}", def.name));
            }
        }
    }
    out
}
