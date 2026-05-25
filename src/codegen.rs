use std::path::Path;

use proc_macro2::TokenStream;
use quote::quote;
use serde_json::{json, Value};

/// Intermediate representation for a single witness field.
#[derive(Debug)]
pub struct FieldMeta {
    pub name: String,
    pub idl_type: &'static str,
    pub required: bool,
    pub description: Option<String>,
}

/// Build the IDL JSON value from a slice of field metadata.
///
/// Produces:
/// ```json
/// { "witness": [ { "name": "...", "type": "...", "required": true/false }, ... ] }
/// ```
/// The `"description"` key is included only when `Some`.
/// Field order matches the input slice order.
pub fn build_idl(fields: &[FieldMeta]) -> Value {
    let array: Vec<Value> = fields
        .iter()
        .map(|f| {
            let mut obj = json!({
                "name": f.name,
                "type": f.idl_type,
                "required": f.required,
            });
            if let Some(desc) = &f.description {
                obj["description"] = json!(desc);
            }
            obj
        })
        .collect();

    json!({ "witness": array })
}

/// Emit a `const _CKB_WITNESS_IDL_PATH: &str = "<path>";` token stream.
pub fn emit_const(idl_path: &Path) -> TokenStream {
    let path_str = idl_path.to_string_lossy();
    quote! {
        const _CKB_WITNESS_IDL_PATH: &str = #path_str;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_field(
        name: &str,
        idl_type: &'static str,
        required: bool,
        description: Option<&str>,
    ) -> FieldMeta {
        FieldMeta {
            name: name.to_string(),
            idl_type,
            required,
            description: description.map(|s| s.to_string()),
        }
    }

    // ── Unit tests (task 6.2) ────────────────────────────────────────────────

    #[test]
    fn top_level_witness_key_is_array() {
        let idl = build_idl(&[make_field("sig", "secp256k1_sig", true, None)]);
        assert!(idl["witness"].is_array());
    }

    #[test]
    fn field_count_matches_input() {
        let fields = vec![
            make_field("a", "uint8", true, None),
            make_field("b", "uint32", false, None),
            make_field("c", "bytes", true, Some("blob")),
        ];
        let idl = build_idl(&fields);
        assert_eq!(idl["witness"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn field_order_matches_input() {
        let fields = vec![
            make_field("first", "uint8", true, None),
            make_field("second", "uint64", false, None),
        ];
        let idl = build_idl(&fields);
        let arr = idl["witness"].as_array().unwrap();
        assert_eq!(arr[0]["name"], "first");
        assert_eq!(arr[1]["name"], "second");
    }

    #[test]
    fn description_omitted_when_none() {
        let idl = build_idl(&[make_field("x", "uint8", true, None)]);
        let obj = &idl["witness"][0];
        assert!(obj.get("description").is_none());
    }

    #[test]
    fn description_present_when_some() {
        let idl = build_idl(&[make_field("x", "uint8", true, Some("my desc"))]);
        let obj = &idl["witness"][0];
        assert_eq!(obj["description"], "my desc");
    }

    #[test]
    fn empty_fields_produces_empty_array() {
        let idl = build_idl(&[]);
        assert_eq!(idl["witness"].as_array().unwrap().len(), 0);
    }

    // ── Property tests (tasks 6.3, 6.4, 6.5) ────────────────────────────────

    use proptest::prelude::*;

    /// Strategy: generate a valid IDL type string from the blessed set.
    fn arb_idl_type() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("uint8"),
            Just("uint32"),
            Just("uint64"),
            Just("secp256k1_sig"),
            Just("secp256k1_pubkey"),
            Just("schnorr_sig"),
            Just("bytes"),
        ]
    }

    /// Strategy: generate a FieldMeta with arbitrary name, type, required, description.
    fn arb_field_meta() -> impl Strategy<Value = FieldMeta> {
        (
            "[a-z][a-z0-9_]{0,15}",
            arb_idl_type(),
            any::<bool>(),
            proptest::option::of("[^\x00]{1,64}"),
        )
            .prop_map(|(name, idl_type, required, description)| FieldMeta {
                name,
                idl_type,
                required,
                description,
            })
    }

    proptest! {
        // Property 3: IDL structural invariants
        #[test]
        fn prop3_idl_structural_invariants(
            fields in proptest::collection::vec(arb_field_meta(), 0..=20)
        ) {
            let n = fields.len();
            let idl = build_idl(&fields);

            // Top-level "witness" key is an array
            let arr = idl["witness"].as_array()
                .expect("\"witness\" must be a JSON array");

            // Array length equals input length
            prop_assert_eq!(arr.len(), n);

            // Each element has "name" (string), "type" (string), "required" (bool)
            // and order matches input
            for (i, (elem, field)) in arr.iter().zip(fields.iter()).enumerate() {
                prop_assert!(elem["name"].is_string(),
                    "element {i} missing string \"name\"");
                prop_assert!(elem["type"].is_string(),
                    "element {i} missing string \"type\"");
                prop_assert!(elem["required"].is_boolean(),
                    "element {i} missing boolean \"required\"");
                // Order check
                prop_assert_eq!(elem["name"].as_str().unwrap(), field.name.as_str());
            }
        }

        // Property 1: Required flag fidelity
        #[test]
        fn prop1_required_flag_fidelity(
            inputs in proptest::collection::vec(
                (any::<bool>(), proptest::option::of("[^\x00]{1,64}")),
                1..=20
            )
        ) {
            let fields: Vec<FieldMeta> = inputs
                .iter()
                .enumerate()
                .map(|(i, (req, desc))| FieldMeta {
                    name: format!("field_{i}"),
                    idl_type: "uint8",
                    required: *req,
                    description: desc.clone(),
                })
                .collect();

            let idl = build_idl(&fields);
            let arr = idl["witness"].as_array().unwrap();

            for (elem, (req, _)) in arr.iter().zip(inputs.iter()) {
                prop_assert_eq!(
                    elem["required"].as_bool().unwrap(),
                    *req,
                    "required flag mismatch"
                );
            }
        }

        // Property 2: Description round-trip
        #[test]
        fn prop2_description_round_trip(
            desc in "[^\x00]{1,128}"
        ) {
            let fields = vec![FieldMeta {
                name: "x".to_string(),
                idl_type: "uint8",
                required: true,
                description: Some(desc.clone()),
            }];

            let idl = build_idl(&fields);
            let got = idl["witness"][0]["description"]
                .as_str()
                .expect("description must be a string");

            prop_assert_eq!(got, desc.as_str());
        }

        // Property 5: IDL JSON round-trip (task 9.1)
        // build_idl → to_string → from_str → to_string again must be byte-for-byte identical.
        #[test]
        fn prop5_idl_json_round_trip(
            fields in proptest::collection::vec(arb_field_meta(), 0..=20)
        ) {
            let idl = build_idl(&fields);
            let first = serde_json::to_string(&idl)
                .expect("first serialisation must succeed");
            let reparsed: serde_json::Value = serde_json::from_str(&first)
                .expect("re-parse must succeed");
            let second = serde_json::to_string(&reparsed)
                .expect("second serialisation must succeed");

            prop_assert_eq!(&first, &second, "round-trip serialisation mismatch");
        }

        // Property 6: Serialisation format consistency (task 9.2)
        // Two independent IDL documents must use the same format (both compact or both pretty).
        #[test]
        fn prop6_serialisation_format_consistency(
            fields_a in proptest::collection::vec(arb_field_meta(), 0..=10),
            fields_b in proptest::collection::vec(arb_field_meta(), 0..=10),
        ) {
            let json_a = serde_json::to_string(&build_idl(&fields_a))
                .expect("serialisation of a must succeed");
            let json_b = serde_json::to_string(&build_idl(&fields_b))
                .expect("serialisation of b must succeed");

            // Compact JSON has no newlines; pretty-printed JSON does.
            let a_has_newlines = json_a.contains('\n');
            let b_has_newlines = json_b.contains('\n');

            prop_assert_eq!(
                a_has_newlines, b_has_newlines,
                "format mismatch: one output has newlines and the other does not"
            );
        }
    }
}
