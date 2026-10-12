//! Kotlin types for the Android client (`mobile/shared`), generated from the
//! same Rust API types as the TypeScript ones: schemars derives a JSON Schema
//! of each type (it reads the serde attributes), [`model`] turns the schemas
//! into a small model and [`emit`] writes kotlinx.serialization classes into
//! `mobile/shared/src/commonMain/.../api/generated/`.
//!
//! Contract samples ([`samples`]): JSON written by the real Rust types for
//! every variant of every enum. They are checked here against the model
//! ([`coverage`]: every variant covered, no key the model lacks, every key the
//! model says is always written present) and written to the Kotlin tests
//! (`ContractSamples.kt`), which read each one and write it back unchanged.
//!
//! `generated_kotlin_is_up_to_date` fails when the committed Kotlin files
//! differ from what the Rust types produce. Regenerate with `pnpm gen:kotlin`
//! (`YHTYE_UPDATE_KOTLIN=1 cargo test -p yhtye-core --lib api::typegen::kotlin`).

mod coverage;
mod emit;
mod model;
mod samples;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use schemars::generate::{Contract, SchemaSettings};
use serde_json::{Map, Value};

use crate::api::{
    ApiCommand, ApiError, ApiEvent, ApiResponse, WsReply, WsRequest, WsServerMessage,
};
use crate::remote::{PairRequest, PairResponse, RemoteError};
use model::{Body, Model};

const PACKAGE_DIR: &str = "kotlin/io/github/tom039224/yhtye/shared/api/generated";

fn mobile_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mobile/shared/src")
}

fn main_dir() -> PathBuf {
    mobile_dir().join("commonMain").join(PACKAGE_DIR)
}

fn test_dir() -> PathBuf {
    mobile_dir().join("commonTest").join(PACKAGE_DIR)
}

/// The `$defs` of every API type under one contract.
fn definitions(contract: Contract) -> Map<String, Value> {
    fn add<T: JsonSchema>(g: &mut schemars::SchemaGenerator) {
        g.subschema_for::<T>();
    }
    let mut settings = SchemaSettings::draft2020_12();
    settings.contract = contract;
    let mut g = settings.into_generator();
    add::<ApiCommand>(&mut g);
    add::<ApiResponse>(&mut g);
    add::<ApiError>(&mut g);
    add::<ApiEvent>(&mut g);
    add::<WsRequest>(&mut g);
    add::<WsReply>(&mut g);
    add::<WsServerMessage>(&mut g);
    add::<PairRequest>(&mut g);
    add::<PairResponse>(&mut g);
    add::<RemoteError>(&mut g);
    g.take_definitions(true)
}

fn model() -> Model {
    model::build(
        &definitions(Contract::Serialize),
        &definitions(Contract::Deserialize),
    )
}

/// Every generated file: `main/<Name>.kt` and `test/ContractSamples.kt`.
fn generate() -> BTreeMap<String, String> {
    let model = model();
    let mut files: BTreeMap<String, String> = model
        .values()
        .map(|def| (format!("main/{}.kt", def.name), emit::file(&model, def)))
        .collect();
    let samples = samples::all();
    files.insert(
        "test/ContractSamples.kt".into(),
        samples_file(&model, &samples),
    );
    files
}

fn samples_file(model: &Model, samples: &[samples::Sample]) -> String {
    let mut text = String::from(emit::HEADER);
    text.push_str(&format!(
        "\npackage {}\n\nimport kotlinx.serialization.KSerializer\n\n",
        emit::PACKAGE
    ));
    text.push_str(
        "/** JSON written by the Rust core (`samples.rs`) and the type it is read as. */\n\
         internal class ContractSample(\n    val type: String,\n    val name: String,\n    \
         val serializer: KSerializer<*>,\n    val json: String,\n)\n\n\
         internal val contractSamples: List<ContractSample> = listOf(\n",
    );
    for s in samples {
        let serializer = match model[s.ty].body {
            Body::Struct(_) => format!("{}.serializer()", s.ty),
            _ => format!("{}.Serializer", s.ty),
        };
        let json = serde_json::to_string(&s.json).expect("json");
        text.push_str(&format!(
            "    ContractSample(\n        {},\n        {},\n        {serializer},\n        {},\n    ),\n",
            emit::string_literal(s.ty),
            emit::string_literal(&s.name),
            emit::string_literal(&json),
        ));
    }
    text.push_str(")\n");
    text
}

fn read_dir(dir: &Path, prefix: &str, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.map(|e| e.expect("dir entry").path()) {
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert!(path.is_file(), "unexpected {}", path.display());
        let text = std::fs::read_to_string(&path).expect("read generated file");
        files.insert(format!("{prefix}/{name}"), text);
    }
}

fn write_dir(dir: &Path, prefix: &str, files: &BTreeMap<String, String>) {
    if dir.exists() {
        std::fs::remove_dir_all(dir).expect("remove the old generated files");
    }
    std::fs::create_dir_all(dir).expect("create dir");
    for (name, text) in files {
        if let Some(file) = name.strip_prefix(&format!("{prefix}/")) {
            std::fs::write(dir.join(file), text).expect("write generated file");
        }
    }
}

#[test]
fn generated_kotlin_is_up_to_date() {
    let fresh = generate();
    if std::env::var_os("YHTYE_UPDATE_KOTLIN").is_some() {
        write_dir(&main_dir(), "main", &fresh);
        write_dir(&test_dir(), "test", &fresh);
        return;
    }
    let mut current = BTreeMap::new();
    read_dir(&main_dir(), "main", &mut current);
    read_dir(&test_dir(), "test", &mut current);
    let stale: std::collections::BTreeSet<&String> = fresh
        .keys()
        .chain(current.keys())
        .filter(|name| fresh.get(*name) != current.get(*name))
        .collect();
    assert!(
        stale.is_empty(),
        "the generated Kotlin API types are out of date ({stale:?}); run `pnpm gen:kotlin`"
    );
}

#[test]
fn contract_samples_cover_every_variant_and_match_the_model() {
    let model = model();
    let samples = samples::all();
    let mut seen = coverage::Seen::default();
    for s in &samples {
        coverage::walk(
            &model,
            &mut seen,
            s.ty,
            &s.json,
            &format!("{}/{}", s.ty, s.name),
        );
    }
    let missing = coverage::missing(&model, &seen);
    assert!(
        missing.is_empty(),
        "no contract sample has these variants (add them to samples.rs): {missing:#?}"
    );
    let mut names = std::collections::BTreeSet::new();
    for s in &samples {
        assert!(
            names.insert((s.ty, &s.name)),
            "sample {}/{} twice",
            s.ty,
            s.name
        );
    }
}

/// Every `#[serde(...)]` key (and `skip_serializing_if` predicate) in the files
/// of the API types must be one the generator was checked with: another one
/// may change the JSON in a way the schema does not show.
#[test]
fn api_types_use_only_supported_serde_attributes() {
    const KEYS: &[&str] = &[
        "tag",
        "content",
        "rename_all",
        "rename",
        "default",
        "skip_serializing_if",
        "deny_unknown_fields",
        "untagged",
    ];
    const SKIP_IF: &[&str] = &["Option::is_none", "Vec::is_empty"];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    let mut bad = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).expect("read");
        let api_types = text.contains("JsonSchema, TS") || path.ends_with("remote/server.rs");
        if !api_types || path.components().any(|c| c.as_os_str() == "typegen") {
            continue;
        }
        for attr in text.split("#[serde(").skip(1) {
            let attr = &attr[..attr.find(")]").expect("attribute end")];
            for part in attr.split(',') {
                let (key, value) = part.split_once('=').unwrap_or((part, ""));
                let (key, value) = (key.trim(), value.trim().trim_matches('"'));
                if !KEYS.contains(&key)
                    || (key == "skip_serializing_if" && !SKIP_IF.contains(&value))
                {
                    bad.push(format!("{}: {}", path.display(), part.trim()));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "serde attributes not checked with the Kotlin generator (check the schema, \
         the generated Kotlin and the samples, then allow them here): {bad:#?}"
    );
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for path in std::fs::read_dir(dir)
        .expect("src")
        .map(|e| e.expect("entry").path())
    {
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}
