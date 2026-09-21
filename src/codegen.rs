use std::path::Path;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::{Value, json};

use crate::registry::WireKind;

/// Intermediate representation for a single witness field.
#[derive(Debug)]
pub struct FieldMeta {
    pub name: String,
    /// Structural IDL type derived from the Rust type (e.g. `"bytes_fixed_65"`, `"uint64"`).
    pub idl_type: String,
    pub required: bool,
    pub description: Option<String>,
    /// Optional semantic label supplied via `#[witness(type = "...")]`.
    ///
    /// When `Some`, this overrides `idl_type` in the generated IDL JSON.
    /// The wire encoding is always driven by `wire_kind` — the label is
    /// purely descriptive and does not affect runtime behaviour.
    pub type_override: Option<String>,
    /// How this field is encoded on the wire — drives `from_witness_args` codegen.
    pub wire_kind: WireKind,
}

/// Build the IDL JSON value from a slice of field metadata.
///
/// The `"type"` value in each field object is:
/// - `field.type_override` if present (user-supplied semantic label), or
/// - `field.idl_type` otherwise (structural default, e.g. `"bytes_fixed_65"`).
///
/// Produces:
/// ```json
/// {
///   "idl_version": "1",
///   "encoding": {
///     "variable_length_prefix": { "width": 4, "endian": "little" },
///     "optional_fields": "trailing_exhaustion"
///   },
///   "witness": [ { "name": "...", "type": "...", "required": true/false }, ... ]
/// }
/// ```
/// The `"description"` key is included only when `Some`.
/// Field order matches the input slice order.
pub fn build_idl(fields: &[FieldMeta]) -> Value {
    let array: Vec<Value> = fields
        .iter()
        .map(|f| {
            let type_str = f.type_override.as_deref().unwrap_or(f.idl_type.as_str());
            let mut obj = json!({
                "name": f.name,
                "type": type_str,
                "required": f.required,
            });
            if let Some(desc) = &f.description {
                obj["description"] = json!(desc);
            }
            obj
        })
        .collect();

    json!({
        "idl_version": "1",
        "encoding": {
            "variable_length_prefix": { "width": 4, "endian": "little" },
            "optional_fields": "trailing_exhaustion"
        },
        "witness": array
    })
}

/// Emit a `pub const _CKB_WITNESS_IDL_PATH: &str = "<path>";` token stream.
pub fn emit_const(idl_path: &Path) -> TokenStream {
    let path_str = idl_path.to_string_lossy();
    quote! {
        pub const _CKB_WITNESS_IDL_PATH: &str = #path_str;
    }
}

/// Emit the `WitnessFields` trait impl for a `#[derive(CkbInnerWitness)]` struct.
///
/// This generates:
/// - `idl_fields()` returning a static slice of `FieldSpec` built from the field metadata.
/// - `decode_fields(buf, cursor)` using the same decode stmts as `emit_impl`, but
///   operating on a caller-supplied buffer slice rather than loading from the CKB VM.
///
/// Unlike `emit_impl`, this function does NOT emit `from_witness_args` (no VM syscall)
/// and does NOT check for trailing bytes — the caller controls the cursor and is
/// responsible for consuming exactly the right number of bytes.
pub fn emit_inner_impl(struct_name: &syn::Ident, fields: &[FieldMeta]) -> TokenStream {
    let decode_stmts = emit_decode_stmts(fields);

    let field_idents: Vec<_> = fields.iter().map(|f| format_ident!("{}", f.name)).collect();

    // Build the static FieldSpec slice entries.
    let field_specs: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let name_str = &f.name;
            let type_str = f.type_override.as_deref().unwrap_or(f.idl_type.as_str());
            let required = f.required;
            let desc = match &f.description {
                Some(d) => quote! { ::core::option::Option::Some(#d) },
                None => quote! { ::core::option::Option::None },
            };
            quote! {
                ::ckb_idl_types::FieldSpec {
                    name:        #name_str,
                    idl_type:    #type_str,
                    required:    #required,
                    description: #desc,
                }
            }
        })
        .collect();

    quote! {
        impl ::ckb_idl_types::WitnessFields for #struct_name {
            fn idl_fields() -> &'static [::ckb_idl_types::FieldSpec] {
                &[ #(#field_specs),* ]
            }

            fn decode_fields(
                buf: &[u8],
                __cursor_ref: &mut usize,
            ) -> ::core::result::Result<Self, ::ckb_idl_types::WitnessError> {
                // Decode stmts use `cursor` as a plain `usize` via `&mut cursor`.
                // We read from __cursor_ref, run the stmts, then write back.
                let mut cursor: usize = *__cursor_ref;
                #(#decode_stmts)*
                *__cursor_ref = cursor;
                ::core::result::Result::Ok(Self {
                    #(#field_idents),*
                })
            }
        }
    }
}

/// Extract the per-field decode statements shared by both `emit_impl` and
/// `emit_inner_impl`. Both functions need identical decode logic — keeping it
/// here ensures any fix is applied in one place.
fn emit_decode_stmts(fields: &[FieldMeta]) -> Vec<TokenStream> {
    fields
        .iter()
        .map(|f| {
            let ident = format_ident!("{}", f.name);
            let name_str = &f.name;

            match &f.wire_kind {
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

                WireKind::FixedScalar { size: 2 } => quote! {
                    let #ident: u16 = {
                        if cursor + 2 > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 2,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u16::from_le_bytes(buf[cursor..cursor + 2].try_into().unwrap());
                        cursor += 2;
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

                WireKind::FixedScalar { size: 16 } => quote! {
                    let #ident: u128 = {
                        if cursor + 16 > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 16,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u128::from_le_bytes(buf[cursor..cursor + 16].try_into().unwrap());
                        cursor += 16;
                        v
                    };
                },

                WireKind::FixedScalar { size: _ } => {
                    // map_wire_kind only emits size 1/2/4/8/16; anything else is a bug.
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

                WireKind::Optional(inner) => match inner.as_ref() {
                    WireKind::VarBytes => quote! {
                        let #ident: ::core::option::Option<::alloc::vec::Vec<u8>> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else {
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
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedArray { size } => quote! {
                        let #ident: ::core::option::Option<[u8; #size]> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else {
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
                                ::core::option::Option::Some(arr)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 1 } => quote! {
                        let #ident: ::core::option::Option<u8> = {
                            if cursor >= buf.len() { ::core::option::Option::None }
                            else { cursor += 1; ::core::option::Option::Some(buf[cursor - 1]) }
                        };
                    },
                    WireKind::FixedScalar { size: 2 } => quote! {
                        let #ident: ::core::option::Option<u16> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if cursor + 2 > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 2,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u16::from_le_bytes(buf[cursor..cursor+2].try_into().unwrap());
                                cursor += 2;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 4 } => quote! {
                        let #ident: ::core::option::Option<u32> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if cursor + 4 > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 4,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u32::from_le_bytes(buf[cursor..cursor+4].try_into().unwrap());
                                cursor += 4;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 8 } => quote! {
                        let #ident: ::core::option::Option<u64> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if cursor + 8 > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 8,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u64::from_le_bytes(buf[cursor..cursor+8].try_into().unwrap());
                                cursor += 8;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 16 } => quote! {
                        let #ident: ::core::option::Option<u128> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if cursor + 16 > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 16,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u128::from_le_bytes(buf[cursor..cursor+16].try_into().unwrap());
                                cursor += 16;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    _ => quote! { compile_error!("unsupported Optional inner kind in CkbWitness codegen"); },
                },

                WireKind::Struct(type_path) => quote! {
                    let #ident = <#type_path as ::ckb_idl_types::WitnessFields>::decode_fields(
                        buf, &mut cursor
                    )?;
                },
            }
        })
        .collect()
}

/// Emit the `from_witness_args` impl for a `#[derive(CkbWitness)]` struct.
///
/// Loads the witness from the CKB VM, deserialises each field in declaration
/// order, and returns a fully populated struct instance. Trailing bytes after
/// all fields are consumed produce `WitnessError::TrailingBytes`.
pub fn emit_impl(struct_name: &syn::Ident, fields: &[FieldMeta]) -> TokenStream {
    let decode_stmts = emit_decode_stmts(fields);

    // Identifiers for the struct construction expression.
    let field_idents: Vec<_> = fields.iter().map(|f| format_ident!("{}", f.name)).collect();

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
        idl_type: &str,
        required: bool,
        description: Option<&str>,
        wire_kind: WireKind,
    ) -> FieldMeta {
        FieldMeta {
            name: name.to_string(),
            idl_type: idl_type.to_string(),
            required,
            description: description.map(|s| s.to_string()),
            type_override: None,
            wire_kind,
        }
    }

    fn make_field_with_override(
        name: &str,
        idl_type: &str,
        type_override: &str,
        wire_kind: WireKind,
    ) -> FieldMeta {
        FieldMeta {
            name: name.to_string(),
            idl_type: idl_type.to_string(),
            required: true,
            description: None,
            type_override: Some(type_override.to_string()),
            wire_kind,
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn top_level_witness_key_is_array() {
        let idl = build_idl(&[make_field(
            "sig",
            "bytes_fixed_65",
            true,
            None,
            WireKind::FixedArray { size: 65 },
        )]);
        assert!(idl["witness"].is_array());
    }

    #[test]
    fn top_level_idl_version_is_one() {
        let idl = build_idl(&[]);
        assert_eq!(idl["idl_version"].as_str().unwrap(), "1");
    }

    #[test]
    fn top_level_encoding_block_is_present() {
        let idl = build_idl(&[]);
        assert_eq!(
            idl["encoding"]["variable_length_prefix"]["width"]
                .as_u64()
                .unwrap(),
            4
        );
        assert_eq!(
            idl["encoding"]["variable_length_prefix"]["endian"]
                .as_str()
                .unwrap(),
            "little"
        );
        assert_eq!(
            idl["encoding"]["optional_fields"].as_str().unwrap(),
            "trailing_exhaustion"
        );
    }

    #[test]
    fn field_count_matches_input() {
        let fields = vec![
            make_field("a", "uint8", true, None, WireKind::FixedScalar { size: 1 }),
            make_field(
                "b",
                "uint32",
                false,
                None,
                WireKind::FixedScalar { size: 4 },
            ),
            make_field("c", "bytes", true, Some("blob"), WireKind::VarBytes),
        ];
        assert_eq!(build_idl(&fields)["witness"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn field_order_matches_input() {
        let fields = vec![
            make_field(
                "first",
                "uint8",
                true,
                None,
                WireKind::FixedScalar { size: 1 },
            ),
            make_field(
                "second",
                "uint64",
                false,
                None,
                WireKind::FixedScalar { size: 8 },
            ),
        ];
        let arr = build_idl(&fields)["witness"].as_array().unwrap().clone();
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
        assert!(idl["witness"][0].get("description").is_none());
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
        assert_eq!(idl["witness"][0]["description"], "my desc");
    }

    #[test]
    fn empty_fields_produces_empty_array() {
        assert_eq!(build_idl(&[])["witness"].as_array().unwrap().len(), 0);
    }

    // ── type_override tests ───────────────────────────────────────────────────

    #[test]
    fn type_override_replaces_structural_type_in_idl() {
        let idl = build_idl(&[make_field_with_override(
            "sig",
            "bytes_fixed_65",
            "secp256k1_sig",
            WireKind::FixedArray { size: 65 },
        )]);
        // IDL should show the semantic label, not the structural default.
        assert_eq!(idl["witness"][0]["type"], "secp256k1_sig");
    }

    #[test]
    fn no_override_uses_structural_type() {
        let idl = build_idl(&[make_field(
            "sig",
            "bytes_fixed_65",
            true,
            None,
            WireKind::FixedArray { size: 65 },
        )]);
        assert_eq!(idl["witness"][0]["type"], "bytes_fixed_65");
    }

    #[test]
    fn type_override_arbitrary_label() {
        let idl = build_idl(&[make_field_with_override(
            "proof",
            "bytes_fixed_32",
            "my_custom_proof",
            WireKind::FixedArray { size: 32 },
        )]);
        assert_eq!(idl["witness"][0]["type"], "my_custom_proof");
    }

    #[test]
    fn type_override_does_not_affect_wire_kind() {
        // Both fields have [u8; 64] wire encoding regardless of label.
        let fields = vec![
            make_field_with_override(
                "a",
                "bytes_fixed_64",
                "schnorr_sig",
                WireKind::FixedArray { size: 64 },
            ),
            make_field_with_override(
                "b",
                "bytes_fixed_64",
                "bls12_381_half",
                WireKind::FixedArray { size: 64 },
            ),
        ];
        let idl = build_idl(&fields);
        assert_eq!(idl["witness"][0]["type"], "schnorr_sig");
        assert_eq!(idl["witness"][1]["type"], "bls12_381_half");
        // wire_kind is unchanged — both would decode 64 bytes identically.
    }

    // ── Property tests ────────────────────────────────────────────────────────

    use proptest::prelude::*;

    fn arb_idl_type() -> impl Strategy<Value = (String, WireKind)> {
        prop_oneof![
            Just(("uint8".to_string(), WireKind::FixedScalar { size: 1 })),
            Just(("uint32".to_string(), WireKind::FixedScalar { size: 4 })),
            Just(("uint64".to_string(), WireKind::FixedScalar { size: 8 })),
            Just((
                "bytes_fixed_65".to_string(),
                WireKind::FixedArray { size: 65 }
            )),
            Just((
                "bytes_fixed_33".to_string(),
                WireKind::FixedArray { size: 33 }
            )),
            Just((
                "bytes_fixed_64".to_string(),
                WireKind::FixedArray { size: 64 }
            )),
            Just((
                "bytes_fixed_32".to_string(),
                WireKind::FixedArray { size: 32 }
            )),
            Just(("bytes".to_string(), WireKind::VarBytes)),
        ]
    }

    fn arb_field_meta() -> impl Strategy<Value = FieldMeta> {
        (
            "[a-z][a-z0-9_]{0,15}",
            arb_idl_type(),
            any::<bool>(),
            proptest::option::of("[^\x00]{1,64}"),
            proptest::option::of("[a-z][a-z0-9_]{0,20}"),
        )
            .prop_map(
                |(name, (idl_type, wire_kind), required, description, type_override)| FieldMeta {
                    name,
                    idl_type,
                    required,
                    description,
                    type_override,
                    wire_kind,
                },
            )
    }

    proptest! {
        #[test]
        fn prop3_idl_structural_invariants(
            fields in proptest::collection::vec(arb_field_meta(), 0..=20)
        ) {
            let n = fields.len();
            let idl = build_idl(&fields);

            // Top-level shape
            prop_assert_eq!(idl["idl_version"].as_str().unwrap(), "1");
            prop_assert_eq!(idl["encoding"]["variable_length_prefix"]["width"].as_u64().unwrap(), 4);
            prop_assert_eq!(idl["encoding"]["variable_length_prefix"]["endian"].as_str().unwrap(), "little");
            prop_assert_eq!(idl["encoding"]["optional_fields"].as_str().unwrap(), "trailing_exhaustion");

            let arr = idl["witness"].as_array()
                .expect("\"witness\" must be a JSON array");

            prop_assert_eq!(arr.len(), n);

            for (i, (elem, field)) in arr.iter().zip(fields.iter()).enumerate() {
                prop_assert!(elem["name"].is_string(),      "element {i} missing string \"name\"");
                prop_assert!(elem["type"].is_string(),      "element {i} missing string \"type\"");
                prop_assert!(elem["required"].is_boolean(), "element {i} missing boolean \"required\"");
                prop_assert_eq!(elem["name"].as_str().unwrap(), field.name.as_str());

                // type must be the override when present, structural otherwise
                let expected_type = field.type_override.as_deref()
                    .unwrap_or(field.idl_type.as_str());
                prop_assert_eq!(elem["type"].as_str().unwrap(), expected_type);
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
                    idl_type: "uint8".to_string(),
                    required: *req,
                    description: desc.clone(),
                    type_override: None,
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
                idl_type: "uint8".to_string(),
                required: true,
                description: Some(desc.clone()),
                type_override: None,
                wire_kind: WireKind::FixedScalar { size: 1 },
            }];

            let got = build_idl(&fields)["witness"][0]["description"]
                .as_str()
                .expect("description must be a string")
                .to_string();

            prop_assert_eq!(got, desc);
        }

        #[test]
        fn prop5_idl_json_round_trip(
            fields in proptest::collection::vec(arb_field_meta(), 0..=20)
        ) {
            let idl = build_idl(&fields);
            let first    = serde_json::to_string(&idl).expect("first serialisation must succeed");
            let reparsed: serde_json::Value = serde_json::from_str(&first).expect("re-parse must succeed");
            let second   = serde_json::to_string(&reparsed).expect("second serialisation must succeed");
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
