//! Generating the web client's response validators from the Rust structs.
//!
//! `apps/web/src/lib/api.ts` used to declare every response shape by hand — two
//! dozen mirrors of Rust structs, kept in step by nobody. Once those shapes
//! became runtime validators, a mirror that disagreed with its source stopped
//! being cosmetic and became a refused response: too strict and a valid payload
//! fails, too loose and a renamed field reaches the DOM as `undefined`.
//!
//! So they are emitted from the schemas instead. What is generated is not a set
//! of types sitting beside a hand-written validator — it *is* the validator,
//! with the TypeScript type inferred from it. One declaration, and Rust owns it.
//!
//! Test-only: none of this ships in the binary. It is a test rather than a
//! build script because its job is to be able to *fail* — to notice, on the
//! commit that changes a struct, that the checked-in file no longer matches.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use schemars::{
    gen::{SchemaGenerator, SchemaSettings},
    schema::{InstanceType, Schema, SchemaObject, SingleOrVec},
    Map,
};

/// The generated file, relative to the repo root.
const GENERATED: &str = "apps/web/src/lib/api-shapes.generated.ts";

/// Set to rewrite the generated file instead of asserting it is current.
const UPDATE_VAR: &str = "KATAAN_UPDATE_SHAPES";

/// The responses the web client reads: every type a handler returns as
/// `Json<_>`.
///
/// Listing them is the whole configuration. Their field types are pulled in
/// transitively, so a new nested struct needs no entry here — only a new
/// *endpoint* does.
fn roots(generator: &mut SchemaGenerator) -> Vec<Schema> {
    macro_rules! roots {
        ($($ty:ty),* $(,)?) => { vec![$(generator.subschema_for::<$ty>()),*] };
    }

    use crate::api::{
        CanonicalFolderResponse, DocumentResponse, FileResponse, FoldersResponse,
        HighlightResponse, OkResponse, ValidateResponse,
    };
    use kataan_core::{
        index::VaultConfig,
        schema::{OntologyResponse, TomlSchemaResponse},
        vault::ResolvedDocument,
    };
    use kataan_search::{ReindexResponse, SearchResponse, SearchStatus};

    roots![
        VaultConfig,
        OntologyResponse,
        FoldersResponse,
        CanonicalFolderResponse,
        DocumentResponse,
        FileResponse,
        HighlightResponse,
        ResolvedDocument,
        ValidateResponse,
        TomlSchemaResponse,
        SearchResponse,
        SearchStatus,
        ReindexResponse,
        OkResponse,
    ]
}

/// Rust names the client does not use.
///
/// A rename is a deliberate act: the client calls a `VaultConfig` the vault
/// *index*, and the `…Response` suffix on a struct nested inside another
/// response says nothing to a reader of `folder.documents[0]`. Renaming a Rust
/// struct should not silently rename an export the app imports, so the mapping
/// is written down rather than derived.
const RENAMES: &[(&str, &str)] = &[
    ("VaultConfig", "VaultIndex"),
    ("FolderSummaryResponse", "FolderSummary"),
    ("FolderChildResponse", "FolderChild"),
    ("FolderDocumentResponse", "FolderDocument"),
    ("FolderFileResponse", "FolderFile"),
    ("DiagnosticResponse", "Diagnostic"),
    ("ResolvedDocument", "ResolveResponse"),
    ("ReindexResponse", "SearchReindexResponse"),
    ("Kind", "SearchResultKind"),
];

fn generator() -> SchemaGenerator {
    SchemaSettings::draft07()
        .with(|settings| {
            // `Option<T>` as `T | null` rather than a nullable flag, so one rule
            // recognises optionality wherever it turns up.
            settings.option_nullable = false;
            settings.option_add_null_type = true;
            settings.inline_subschemas = false;
        })
        .into_generator()
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("crates/<crate> lives two levels below the repo root")
        .to_path_buf()
}

#[test]
fn generated_shapes_match_the_rust_types() {
    let generated = emit();
    let path = repo_root().join(GENERATED);

    if std::env::var_os(UPDATE_VAR).is_some() {
        std::fs::write(&path, &generated).expect("write the generated shapes");
        return;
    }

    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current != generated {
        panic!(
            "{GENERATED} no longer matches the Rust response types.\n\n\
             A response struct changed, so the client's validator has to change with \
             it. Regenerate:\n\n    {UPDATE_VAR}=1 cargo test -p kataan-server shapes\n\n\
             Then check the diff: a field that disappeared or changed type is a \
             breaking change for the web app, and `astro check` will say where.\n"
        );
    }
}

fn emit() -> String {
    let mut generator = generator();
    let roots = roots(&mut generator);
    let definitions = generator.take_definitions();

    let root_names: BTreeSet<String> = roots.iter().filter_map(reference_name).collect();
    Emitter::new(&definitions, root_names).run()
}

/// The definition a schema is a bare `$ref` to, if that is all it is.
fn reference_name(schema: &Schema) -> Option<String> {
    let reference = match schema {
        Schema::Object(object) => object.reference.as_deref()?,
        Schema::Bool(_) => return None,
    };
    reference
        .strip_prefix("#/definitions/")
        .map(ToOwned::to_owned)
}

struct Emitter<'a> {
    definitions: &'a Map<String, Schema>,
    /// Definitions reachable from a response, in an order where a shape is
    /// declared after everything it refers to.
    order: Vec<String>,
    /// Definitions that refer to themselves, directly or through others. These
    /// cannot be a plain `const` — the shape has to be built lazily and its
    /// TypeScript type written out rather than inferred.
    cyclic: BTreeSet<String>,
    /// Which `shape.ts` combinators the output actually uses, so the import
    /// list carries nothing dead.
    used: BTreeSet<&'static str>,
    roots: BTreeSet<String>,
}

impl<'a> Emitter<'a> {
    fn new(definitions: &'a Map<String, Schema>, roots: BTreeSet<String>) -> Self {
        let mut emitter = Self {
            definitions,
            order: Vec::new(),
            cyclic: BTreeSet::new(),
            used: BTreeSet::new(),
            roots,
        };
        emitter.plan();
        emitter
    }

    /// Order the definitions by dependency and find the ones on a cycle.
    fn plan(&mut self) {
        let mut visited = BTreeSet::new();
        let mut stack = Vec::new();
        let roots: Vec<String> = self.roots.iter().cloned().collect();
        for name in roots {
            self.visit(&name, &mut visited, &mut stack);
        }
    }

    fn visit(&mut self, name: &str, visited: &mut BTreeSet<String>, stack: &mut Vec<String>) {
        if stack.iter().any(|entry| entry == name) {
            // A back edge: everything from the first sighting to here is on a
            // cycle with `name`.
            let start = stack.iter().position(|entry| entry == name).unwrap_or(0);
            for entry in &stack[start..] {
                self.cyclic.insert(entry.clone());
            }
            self.cyclic.insert(name.to_owned());
            return;
        }
        if !visited.insert(name.to_owned()) {
            return;
        }
        stack.push(name.to_owned());
        if let Some(schema) = self.definitions.get(name) {
            for referenced in references(schema) {
                self.visit(&referenced, visited, stack);
            }
        }
        stack.pop();
        self.order.push(name.to_owned());
    }

    fn run(mut self) -> String {
        let mut body = String::new();
        for name in self.order.clone() {
            let Some(schema) = self.definitions.get(&name) else {
                continue;
            };
            let ts_name = ts_name(&name);
            let const_name = camel(&ts_name);

            if self.cyclic.contains(&name) {
                // Written out rather than inferred: a type that mentions itself
                // cannot be derived from a value that mentions itself.
                self.used.insert("Shape");
                let ty = self.ts_type(schema, 0);
                let shape = self.shape(schema).replace('\n', "\n  ");
                let _ = writeln!(body, "\nexport type {ts_name} = {ty};");
                let _ = writeln!(
                    body,
                    "export const {const_name}: Shape<{ts_name}> = (value, path) =>\n  \
                     {shape}(value, path) as {ts_name};"
                );
            } else {
                let shape = self.shape(schema);
                let _ = writeln!(body, "\nexport const {const_name} = {shape};");
                let _ = writeln!(body, "export type {ts_name} = Infer<typeof {const_name}>;");
            }
        }

        self.used.insert("Infer");
        let mut imports: Vec<String> = self
            .used
            .iter()
            .map(|name| {
                if *name == "Infer" || *name == "Shape" {
                    format!("type {name}")
                } else {
                    (*name).to_owned()
                }
            })
            .collect();
        imports.sort();

        let mut out = String::new();
        out.push_str(HEADER);
        let _ = writeln!(
            out,
            "import {{\n  {},\n}} from './shape';",
            imports.join(",\n  ")
        );
        out.push_str(&body);
        out
    }

    /// The shape expression for a schema.
    fn shape(&mut self, schema: &Schema) -> String {
        let object = match schema {
            // `true` accepts any JSON. It is what a `serde_json::Value` field
            // becomes, and there is nothing to check.
            Schema::Bool(_) => return self.use_of("json"),
            Schema::Object(object) => object,
        };

        if let Some(name) = reference_name(schema) {
            return camel(&ts_name(&name));
        }

        if let Some(inner) = nullable_inner(object) {
            return format!("{}({})", self.use_of("optional"), self.shape(&inner));
        }

        if let Some(values) = string_enum_values(object) {
            let values: Vec<String> = values.iter().map(|value| format!("'{value}'")).collect();
            let literals = self.use_of("literals");
            let one_line = values.join(", ");
            // Enums are the one construct wide enough to run past a readable
            // line; every other shape is already one short call per line.
            return if one_line.len() <= 80 {
                format!("{literals}({one_line})")
            } else {
                format!("{literals}(\n  {},\n)", values.join(",\n  "))
            };
        }

        match single_instance_type(object) {
            Some(InstanceType::String) => self.use_of("string"),
            Some(InstanceType::Number | InstanceType::Integer) => self.use_of("number"),
            Some(InstanceType::Boolean) => self.use_of("boolean"),
            Some(InstanceType::Array) => {
                let items = array_items(object).unwrap_or(Schema::Bool(true));
                format!("{}({})", self.use_of("array"), self.shape(&items))
            }
            Some(InstanceType::Object) => self.object_shape(object),
            _ => self.use_of("json"),
        }
    }

    fn object_shape(&mut self, object: &SchemaObject) -> String {
        let Some(validation) = &object.object else {
            return self.use_of("record");
        };

        if validation.properties.is_empty() {
            return match validation.additional_properties.as_deref() {
                // A map whose values are described: `BTreeMap<String, T>`.
                Some(values @ Schema::Object(_)) => {
                    let inner = self.shape(&values.clone());
                    format!("{}({inner})", self.use_of("mapOf"))
                }
                _ => self.use_of("record"),
            };
        }

        // `additionalProperties: true` is what `#[serde(flatten)]` leaves
        // behind: declared fields, plus whatever else the author wrote. The
        // client has to be able to index those extras, so the type says so.
        let open = matches!(
            validation.additional_properties.as_deref(),
            Some(Schema::Bool(true))
        );
        let combinator = self.use_of(if open { "openObject" } else { "object" });

        let mut fields = String::new();
        for (key, property) in &validation.properties {
            let shape = self.property_shape(property, validation.required.contains(key));
            let _ = writeln!(fields, "  {}: {shape},", quote_key(key));
        }
        format!("{combinator}({{\n{fields}}})")
    }

    /// A property's shape, wrapped in `optional` when the response may not
    /// carry it.
    ///
    /// JSON Schema's `required` answers a question about *deserializing*, which
    /// is not the one a response validator asks. A field with a serde default
    /// is absent from `required` yet always serialized, so treating it as
    /// missable would push a needless `| undefined` through the whole app. What
    /// actually makes a field optional on the wire is being nullable, or having
    /// neither a default nor a requirement.
    fn property_shape(&mut self, schema: &Schema, required: bool) -> String {
        let shape = self.shape(schema);
        let nullable = matches!(schema, Schema::Object(object) if nullable_inner(object).is_some());
        let defaulted = matches!(
            schema,
            Schema::Object(object) if object.metadata.as_ref().is_some_and(|m| m.default.is_some())
        );
        if nullable || required || defaulted {
            shape
        } else {
            format!("{}({shape})", self.use_of("optional"))
        }
    }

    /// The TypeScript type for a schema, written out rather than inferred.
    ///
    /// Only reached for definitions on a cycle, where inference cannot close
    /// the loop.
    fn ts_type(&mut self, schema: &Schema, depth: usize) -> String {
        let object = match schema {
            Schema::Bool(_) => return "unknown".to_owned(),
            Schema::Object(object) => object,
        };

        if let Some(name) = reference_name(schema) {
            return ts_name(&name);
        }
        if let Some(inner) = nullable_inner(object) {
            return format!("{} | undefined", self.ts_type(&inner, depth));
        }
        if let Some(values) = string_enum_values(object) {
            return values
                .iter()
                .map(|value| format!("'{value}'"))
                .collect::<Vec<_>>()
                .join(" | ");
        }

        match single_instance_type(object) {
            Some(InstanceType::String) => "string".to_owned(),
            Some(InstanceType::Number | InstanceType::Integer) => "number".to_owned(),
            Some(InstanceType::Boolean) => "boolean".to_owned(),
            Some(InstanceType::Array) => {
                let items = array_items(object).unwrap_or(Schema::Bool(true));
                format!("{}[]", self.ts_type(&items, depth))
            }
            Some(InstanceType::Object) => self.ts_object_type(object, depth),
            _ => "unknown".to_owned(),
        }
    }

    fn ts_object_type(&mut self, object: &SchemaObject, depth: usize) -> String {
        let Some(validation) = &object.object else {
            return "Record<string, unknown>".to_owned();
        };
        if validation.properties.is_empty() {
            return match validation.additional_properties.as_deref() {
                Some(values @ Schema::Object(_)) => {
                    let inner = self.ts_type(&values.clone(), depth);
                    format!("Record<string, {inner}>")
                }
                _ => "Record<string, unknown>".to_owned(),
            };
        }

        let indent = "  ".repeat(depth + 1);
        let mut fields = String::new();
        for (key, property) in &validation.properties {
            let required = validation.required.contains(key);
            let nullable =
                matches!(property, Schema::Object(object) if nullable_inner(object).is_some());
            let defaulted = matches!(
                property,
                Schema::Object(object)
                    if object.metadata.as_ref().is_some_and(|m| m.default.is_some())
            );
            let ty = self.ts_type(property, depth + 1);
            // `?:` rather than `| undefined`, so the emitted type matches what
            // `Infer` produces for an `optional(...)` field.
            let ty = ty.trim_end_matches(" | undefined").to_owned();
            let marker = if nullable || !(required || defaulted) {
                "?"
            } else {
                ""
            };
            let _ = writeln!(fields, "{indent}{}{marker}: {ty};", quote_key(key));
        }
        let open = matches!(
            validation.additional_properties.as_deref(),
            Some(Schema::Bool(true))
        );
        let closing = "  ".repeat(depth);
        let body = format!("{{\n{fields}{closing}}}");
        if open {
            format!("{body} & Record<string, unknown>")
        } else {
            body
        }
    }

    fn use_of(&mut self, combinator: &'static str) -> String {
        self.used.insert(combinator);
        combinator.to_owned()
    }
}

/// The definitions a schema refers to, at any depth.
fn references(schema: &Schema) -> Vec<String> {
    let mut found = Vec::new();
    walk(schema, &mut found);
    found
}

fn walk(schema: &Schema, found: &mut Vec<String>) {
    let Schema::Object(object) = schema else {
        return;
    };
    if let Some(name) = reference_name(schema) {
        found.push(name);
    }
    if let Some(validation) = &object.object {
        for property in validation.properties.values() {
            walk(property, found);
        }
        if let Some(additional) = validation.additional_properties.as_deref() {
            walk(additional, found);
        }
    }
    if let Some(validation) = &object.array {
        match &validation.items {
            Some(SingleOrVec::Single(item)) => walk(item, found),
            Some(SingleOrVec::Vec(items)) => items.iter().for_each(|item| walk(item, found)),
            None => {}
        }
    }
    if let Some(subschemas) = &object.subschemas {
        for group in [&subschemas.any_of, &subschemas.one_of, &subschemas.all_of] {
            for branch in group.iter().flatten() {
                walk(branch, found);
            }
        }
    }
}

/// The non-null half of an optional schema, however schemars spelled it.
///
/// `Option<String>` becomes `"type": ["string", "null"]`; `Option<Struct>`
/// becomes `anyOf: [{$ref}, {"type": "null"}]`. Both mean the same thing.
fn nullable_inner(object: &SchemaObject) -> Option<Schema> {
    if let Some(SingleOrVec::Vec(types)) = &object.instance_type {
        if types.contains(&InstanceType::Null) && types.len() > 1 {
            let mut inner = object.clone();
            let remaining: Vec<InstanceType> = types
                .iter()
                .copied()
                .filter(|ty| *ty != InstanceType::Null)
                .collect();
            inner.instance_type = Some(match remaining.as_slice() {
                [only] => SingleOrVec::Single(Box::new(*only)),
                many => SingleOrVec::Vec(many.to_vec()),
            });
            return Some(Schema::Object(inner));
        }
    }

    let subschemas = object.subschemas.as_ref()?;
    let branches = subschemas.any_of.as_ref().or(subschemas.one_of.as_ref())?;
    if branches.len() != 2 {
        return None;
    }
    let null_at = branches.iter().position(is_null_schema)?;
    Some(branches[1 - null_at].clone())
}

fn is_null_schema(schema: &Schema) -> bool {
    matches!(
        schema,
        Schema::Object(object)
            if matches!(&object.instance_type, Some(SingleOrVec::Single(ty)) if **ty == InstanceType::Null)
    )
}

/// The string values of a unit-variant enum.
///
/// A derived enum arrives either as one `enum` list or — when a variant carries
/// a doc comment — as a `oneOf` of single-value branches, which describes the
/// same set of strings.
fn string_enum_values(object: &SchemaObject) -> Option<Vec<String>> {
    fn values_of(object: &SchemaObject) -> Option<Vec<String>> {
        let values = object.enum_values.as_ref()?;
        values
            .iter()
            .map(|value| value.as_str().map(ToOwned::to_owned))
            .collect()
    }

    if let Some(values) = values_of(object) {
        return Some(values);
    }

    let subschemas = object.subschemas.as_ref()?;
    let branches = subschemas.one_of.as_ref()?;
    let mut all = Vec::new();
    for branch in branches {
        let Schema::Object(branch) = branch else {
            return None;
        };
        all.extend(values_of(branch)?);
    }
    (!all.is_empty()).then_some(all)
}

fn single_instance_type(object: &SchemaObject) -> Option<InstanceType> {
    match &object.instance_type {
        Some(SingleOrVec::Single(ty)) => Some(**ty),
        Some(SingleOrVec::Vec(types)) if types.len() == 1 => Some(types[0]),
        // No declared type, but properties: schemars omits `type` for some
        // shapes, and an object is what the fields say it is.
        None if object.object.is_some() => Some(InstanceType::Object),
        _ => None,
    }
}

fn array_items(object: &SchemaObject) -> Option<Schema> {
    match &object.array.as_ref()?.items {
        Some(SingleOrVec::Single(item)) => Some((**item).clone()),
        Some(SingleOrVec::Vec(items)) => items.first().cloned(),
        None => None,
    }
}

fn ts_name(rust: &str) -> String {
    RENAMES
        .iter()
        .find(|(from, _)| *from == rust)
        .map(|(_, to)| (*to).to_owned())
        .unwrap_or_else(|| rust.to_owned())
}

fn camel(pascal: &str) -> String {
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Object keys are quoted only when they have to be.
fn quote_key(key: &str) -> String {
    let plain = !key.is_empty()
        && !key.starts_with(|c: char| c.is_ascii_digit())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if plain {
        key.to_owned()
    } else {
        format!("'{key}'")
    }
}

const HEADER: &str = "\
// Generated from the Rust response types. Do not edit.
//
// Every shape below mirrors a struct some handler returns as `Json<_>`. It is
// emitted by `crates/kataan-server/src/shapes.rs`, which is a test: change a
// response struct without regenerating and `cargo test` fails, rather than a
// user's browser.
//
//     KATAAN_UPDATE_SHAPES=1 cargo test -p kataan-server shapes
//
// A shape is both the runtime validator and the source of its TypeScript type,
// so there is nothing here for a human to keep in step.

";

/// Prints the raw schemas. Not part of the suite — run it when the emitter has
/// to be taught a construct schemars produces:
///
/// ```text
/// cargo test -p kataan-server shapes::dump -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn dump() {
    let mut generator = generator();
    let roots = roots(&mut generator);
    let definitions = generator.take_definitions();

    let mut out = String::new();
    let _ = writeln!(out, "=== ROOTS ===");
    for schema in &roots {
        let _ = writeln!(out, "{}", serde_json::to_string(schema).expect("root"));
    }
    let _ = writeln!(out, "\n=== DEFINITIONS ===");
    let named: BTreeMap<_, _> = definitions.iter().collect();
    for (name, schema) in named {
        let _ = writeln!(
            out,
            "{name}: {}",
            serde_json::to_string_pretty(schema).expect("definition")
        );
    }
    std::fs::write(repo_root().join("target/shapes-dump.txt"), &out).expect("write dump");
    println!("{out}");
}
