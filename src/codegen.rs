use std::path::Path;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::{json, Value};

use crate::registry::WireKind;

/// Intermediate representation for a single witness field.
#[derive(Debug)]
pub struct FieldMeta {
    pub name: String,
    pub idl_type: &'static str,
    pub required: bool,
    pub description: Option<String>,
    /// How this field is encoded on the wire — drives `from_witness_args` codegen.
    pub wire_kind: WireKind,
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

/// Emit a `pub const _CKB_WITNESS_IDL_PATH: &str = "<path>";` token stream.
pub fn emit_const(idl_path: &Path) -> TokenStream {
    let path_str = idl_path.to_string_lossy();
    quote! {
        pub const _CKB_WITNESS_IDL_PATH: &str = #path_str;
    }
}

/// Emit the `from_witness_args` impl for the annotated struct.
///
/// Wire format (length-prefixed):
/// - Fixed scalars  (u8/u32/u64)  : read N bytes, decode as little-endian.
/// - Fixed arrays   ([u8; N])      : read exactly N bytes, copy into array.
/// - Variable bytes (Vec<u8>)      : read 4-byte LE length prefix, then that many bytes.
///
/// All reads consume bytes sequentially from the raw `lock` field of
/// `WitnessArgs`. Any leftover bytes after all fields are decoded produce
/// `WitnessError::TrailingBytes`.
pub fn emit_impl(struct_name: &syn::Ident, fields: &[FieldMeta]) -> TokenStream {
    // Build one decode snippet per field.
    let decode_stmts: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let ident = format_ident!("{}", f.name);
            let name_str = &f.name;

            match f.wire_kind {
                WireKind::FixedScalar { size: 1 } => quote! {
                    let #ident: u8 = {
                        if cursor + 1 > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 1,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = buf[cursor];
                        cursor += 1;
                        v
                    };
                },

                WireKind::FixedScalar { size: 4 } => quote! {
                    let #ident: u32 = {
                        if cursor + 4 > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 4,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap());
                        cursor += 4;
                        v
                    };
                },

                WireKind::FixedScalar { size: 8 } => quote! {
                    let #ident: u64 = {
                        if cursor + 8 > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 8,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u64::from_le_bytes(buf[cursor..cursor + 8].try_into().unwrap());
                        cursor += 8;
                        v
                    };
                },

                WireKind::FixedScalar { size: _ } => {
                    // map_wire_kind only emits size 1/4/8; anything else is a bug.
                    quote! { compile_error!("unsupported scalar size in CkbWitness codegen"); }
                }

                WireKind::FixedArray { size } => quote! {
                    let #ident: [u8; #size] = {
                        if cursor + #size > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: #size,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let mut arr = [0u8; #size];
                        arr.copy_from_slice(&buf[cursor..cursor + #size]);
                        cursor += #size;
                        arr
                    };
                },

                WireKind::VarBytes => quote! {
                    let #ident: ::alloc::vec::Vec<u8> = {
                        if cursor + 4 > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 4,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let len = u32::from_le_bytes(
                            buf[cursor..cursor + 4].try_into().unwrap()
                        ) as usize;
                        cursor += 4;
                        if cursor + len > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: len,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = buf[cursor..cursor + len].to_vec();
                        cursor += len;
                        v
                    };
                },
            }
        })
        .collect();

    // Identifiers for the struct construction expression.
    let field_idents: Vec<_> = fields
        .iter()
        .map(|f| format_ident!("{}", f.name))
        .collect();

    quote! {
        impl #struct_name {
            /// Deserialise this witness struct from the `lock` field of
            /// `WitnessArgs` at `(index, source)`.
            ///
            /// Wire format: fixed-size fields are read in declaration order
            /// as little-endian bytes; `Vec<u8>` fields are length-prefixed
            /// (4-byte LE `u32` length followed by that many bytes).
            pub fn from_witness_args(
                index: usize,
                source: ckb_std::ckb_constants::Source,
            ) -> ::core::result::Result<Self, ::ckb_idl_types::WitnessError> {
                use ckb_std::ckb_types::prelude::Unpack;

                let witness_args =
                    ckb_std::high_level::load_witness_args(index, source)
                        .map_err(::ckb_idl_types::WitnessError::Load)?;

                let raw: ckb_std::ckb_types::bytes::Bytes = witness_args
                    .lock()
                    .to_opt()
                    .ok_or(::ckb_idl_types::WitnessError::MissingLockField)?
                    .unpack();

                let buf: &[u8] = raw.as_ref();
                let mut cursor: usize = 0;

                #(#decode_stmts)*

                if cursor != buf.len() {
                    return Err(::ckb_idl_types::WitnessError::TrailingBytes {
                        consumed: cursor,
                        total: buf.len(),
                    });
                }

                ::core::result::Result::Ok(Self {
                    #(#field_idents),*
                })
            }
        }
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
        wire_kind: WireKind,
    ) -> FieldMeta {
        FieldMeta {
            name: name.to_string(),
            idl_type,
            required,
            description: description.map(|s| s.to_string()),
            wire_kind,
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn top_level_witness_key_is_array() {
        let idl = build_idl(&[make_field(
            "sig",
            "secp256k1_sig",
            true,
            None,
            WireKind::FixedArray { size: 65 },
        )]);
        assert!(idl["witness"].is_array());
    }

    #[test]
    fn field_count_matches_input() {
        let fields = vec![
            make_field("a", "uint8", true, None, WireKind::FixedScalar { size: 1 }),
            make_field("b", "uint32", false, None, WireKind::FixedScalar { size: 4 }),
            make_field("c", "bytes", true, Some("blob"), WireKind::VarBytes),
        ];
        let idl = build_idl(&fields);
        assert_eq!(idl["witness"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn field_order_matches_input() {
        let fields = vec![
            make_field("first", "uint8", true, None, WireKind::FixedScalar { size: 1 }),
            make_field("second", "uint64", false, None, WireKind::FixedScalar { size: 8 }),
        ];
        let idl = build_idl(&fields);
        let arr = idl["witness"].as_array().unwrap();
        assert_eq!(arr[0]["name"], "first");
        assert_eq!(arr[1]["name"], "second");
    }

    #[test]
    fn description_omitted_when_none() {
        let idl = build_idl(&[make_field(
            "x",
            "uint8",
            true,
            None,
            WireKind::FixedScalar { size: 1 },
        )]);
        let obj = &idl["witness"][0];
        assert!(obj.get("description").is_none());
    }

    #[test]
    fn description_present_when_some() {
        let idl = build_idl(&[make_field(
            "x",
            "uint8",
            true,
            Some("my desc"),
            WireKind::FixedScalar { size: 1 },
        )]);
        let obj = &idl["witness"][0];
        assert_eq!(obj["description"], "my desc");
    }

    #[test]
    fn empty_fields_produces_empty_array() {
        let idl = build_idl(&[]);
        assert_eq!(idl["witness"].as_array().unwrap().len(), 0);
    }

    // ── Property tests ────────────────────────────────────────────────────────

    use proptest::prelude::*;

    fn arb_idl_type() -> impl Strategy<Value = (&'static str, WireKind)> {
        prop_oneof![
            Just(("uint8",           WireKind::FixedScalar { size: 1 })),
            Just(("uint32",          WireKind::FixedScalar { size: 4 })),
            Just(("uint64",          WireKind::FixedScalar { size: 8 })),
            Just(("secp256k1_sig",   WireKind::FixedArray  { size: 65 })),
            Just(("secp256k1_pubkey",WireKind::FixedArray  { size: 33 })),
            Just(("schnorr_sig",     WireKind::FixedArray  { size: 64 })),
            Just(("bytes",           WireKind::VarBytes)),
        ]
    }

    fn arb_field_meta() -> impl Strategy<Value = FieldMeta> {
        (
            "[a-z][a-z0-9_]{0,15}",
            arb_idl_type(),
            any::<bool>(),
            proptest::option::of("[^\x00]{1,64}"),
        )
            .prop_map(|(name, (idl_type, wire_kind), required, description)| FieldMeta {
                name,
                idl_type,
                required,
                description,
                wire_kind,
            })
    }

    proptest! {
        #[test]
        fn prop3_idl_structural_invariants(
            fields in proptest::collection::vec(arb_field_meta(), 0..=20)
        ) {
            let n = fields.len();
            let idl = build_idl(&fields);

            let arr = idl["witness"].as_array()
                .expect("\"witness\" must be a JSON array");

            prop_assert_eq!(arr.len(), n);

            for (i, (elem, field)) in arr.iter().zip(fields.iter()).enumerate() {
                prop_assert!(elem["name"].is_string(),   "element {i} missing string \"name\"");
                prop_assert!(elem["type"].is_string(),   "element {i} missing string \"type\"");
                prop_assert!(elem["required"].is_boolean(), "element {i} missing boolean \"required\"");
                prop_assert_eq!(elem["name"].as_str().unwrap(), field.name.as_str());
            }
        }

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
                    wire_kind: WireKind::FixedScalar { size: 1 },
                })
                .collect();

            let idl = build_idl(&fields);
            let arr = idl["witness"].as_array().unwrap();

            for (elem, (req, _)) in arr.iter().zip(inputs.iter()) {
                prop_assert_eq!(elem["required"].as_bool().unwrap(), *req);
            }
        }

        #[test]
        fn prop2_description_round_trip(desc in "[^\x00]{1,128}") {
            let fields = vec![FieldMeta {
                name: "x".to_string(),
                idl_type: "uint8",
                required: true,
                description: Some(desc.clone()),
                wire_kind: WireKind::FixedScalar { size: 1 },
            }];

            let idl = build_idl(&fields);
            let got = idl["witness"][0]["description"]
                .as_str()
                .expect("description must be a string");

            prop_assert_eq!(got, desc.as_str());
        }

        #[test]
        fn prop5_idl_json_round_trip(
            fields in proptest::collection::vec(arb_field_meta(), 0..=20)
        ) {
            let idl = build_idl(&fields);
            let first  = serde_json::to_string(&idl).expect("first serialisation must succeed");
            let reparsed: serde_json::Value = serde_json::from_str(&first).expect("re-parse must succeed");
            let second = serde_json::to_string(&reparsed).expect("second serialisation must succeed");
            prop_assert_eq!(&first, &second);
        }

        #[test]
        fn prop6_serialisation_format_consistency(
            fields_a in proptest::collection::vec(arb_field_meta(), 0..=10),
            fields_b in proptest::collection::vec(arb_field_meta(), 0..=10),
        ) {
            let json_a = serde_json::to_string(&build_idl(&fields_a)).expect("a must serialise");
            let json_b = serde_json::to_string(&build_idl(&fields_b)).expect("b must serialise");
            prop_assert_eq!(json_a.contains('\n'), json_b.contains('\n'));
        }
    }
}
