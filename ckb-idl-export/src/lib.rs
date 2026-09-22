//! Host-side LS-IDL 0.1 exporter.
//!
//! Contract derives expose an allocation-free schema graph through
//! `ckb_idl_types`. This crate recursively serialises that graph into the
//! complete JSON artifact that wallets and deployment tooling consume.

use std::{fs, io, path::Path};

use ckb_idl_types::{FieldSchema, StructSchema, TypeSchema, UnionSchema, WitnessSchema};
use serde_json::{Value, json};

pub fn document_for<T: WitnessSchema>() -> Value {
    json!({
        "idl_version": "0.1",
        "encoding": {
            "variable_length_prefix": { "width": 4, "endian": "little" },
            "vector_count_prefix": { "width": 4, "endian": "little" },
            "union_tag": { "width": 4, "endian": "little" },
            "optional_fields": "trailing_exhaustion"
        },
        "witness": fields_json(T::schema().fields),
    })
}

pub fn export_to_path<T: WitnessSchema>(path: impl AsRef<Path>) -> io::Result<()> {
    let bytes = serde_json::to_vec(&document_for::<T>())
        .expect("LS-IDL schema contains only serialisable values");
    fs::write(path, bytes)
}

/// Define a tiny host-side IDL-export binary for a witness type.
///
/// ```ignore
/// // src/bin/export_idl.rs
/// ckb_idl_export::export_idl_main!(my_lock::witness::Witness);
/// ```
///
/// Run it with `cargo run --bin export_idl -- artifacts/idl-0.1.json`.
/// Without an argument it writes `idl-0.1.json` in the current directory.
#[macro_export]
macro_rules! export_idl_main {
    ($witness:ty) => {
        fn main() -> ::std::io::Result<()> {
            let path = ::std::env::args()
                .nth(1)
                .unwrap_or_else(|| "idl-0.1.json".to_owned());
            $crate::export_to_path::<$witness>(path)
        }
    };
}

fn fields_json(fields: &[FieldSchema]) -> Vec<Value> {
    fields.iter().map(field_json).collect()
}

fn field_json(field: &FieldSchema) -> Value {
    let mut value = type_json((field.wire_type)());
    let object = value.as_object_mut().expect("type_json always returns an object");
    object.insert("name".into(), json!(field.name));
    object.insert("required".into(), json!(field.required));
    if let Some(description) = field.description {
        object.insert("description".into(), json!(description));
    }
    if let Some(semantic_type) = field.semantic_type {
        object.insert("semantic_type".into(), json!(semantic_type));
    }
    value
}

fn type_json(ty: &TypeSchema) -> Value {
    match ty {
        TypeSchema::Uint { bits } => json!({ "type": format!("uint{bits}") }),
        TypeSchema::FixedBytes { length } => json!({ "type": format!("bytes_fixed_{length}") }),
        TypeSchema::Bytes => json!({ "type": "bytes" }),
        TypeSchema::Optional { inner } => {
            let mut value = type_json(inner());
            value.as_object_mut().unwrap().insert("optional".into(), json!(true));
            value
        }
        TypeSchema::Vector { element, count_prefix_bits } => json!({
            "type": "vector",
            "count_prefix_bits": count_prefix_bits,
            "items": type_json(element()),
        }),
        TypeSchema::Struct { schema } => struct_json(schema()),
        TypeSchema::Union { schema } => union_json(schema()),
    }
}

fn struct_json(schema: &StructSchema) -> Value {
    json!({ "type": "struct", "fields": fields_json(schema.fields) })
}

fn union_json(schema: &UnionSchema) -> Value {
    let variants: Vec<Value> = schema
        .variants
        .iter()
        .map(|variant| {
            json!({
                "tag": variant.tag,
                "name": variant.name,
                "fields": fields_json((variant.schema)().fields),
            })
        })
        .collect();
    json!({ "type": "union", "variants": variants })
}
