use std::path::Path;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::{Value, json};

use crate::registry::WireKind;

/// Intermediate representation for a single witness field.
#[derive(Debug)]
pub struct FieldMeta {
    /// Original Rust identifier used by generated bindings. This may be a raw
    /// identifier even though `name` contains its unraw IDL representation.
    pub ident: syn::Ident,
    pub name: String,
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
///   "idl_version": "0.1.0",
///   "interfaces": [{
///     "id": "lock_witness", "kind": "witness_args.lock",
///     "encoding": { "id": "ckb-idl-linear-0.1.0" },
///     "fields": [ { "name": "...", "type": "...", "required": true/false } ]
///   }]
/// }
/// ```
/// The `"description"` key is included only when `Some`.
/// Field order matches the input slice order.
pub fn build_idl(fields: &[FieldMeta]) -> Value {
    let array: Vec<Value> = fields
        .iter()
        .map(|f| {
            let structural_type = wire_type_name(&f.wire_kind);
            let effective_type = f.type_override.as_deref().unwrap_or(&structural_type);
            let mut obj = json!({
                "name": f.name,
                "type": effective_type,
                "required": f.required,
            });
            if f.type_override.is_some() {
                obj["wire_type"] = json!(structural_type);
            }
            if let WireKind::VecOf(inner) = &f.wire_kind {
                obj["items"] = json!({ "type": wire_type_name(inner) });
            }
            if let Some(desc) = &f.description {
                obj["description"] = json!(desc);
            }
            obj
        })
        .collect();

    json!({
        "idl_version": "0.1.0",
        "interfaces": [{
            "id": "lock_witness",
            "kind": "witness_args.lock",
            "encoding": { "id": "ckb-idl-linear-0.1.0" },
            "fields": array
        }]
    })
}

fn wire_type_name(kind: &WireKind) -> String {
    match kind {
        WireKind::FixedScalar { size } => format!("uint{}", size * 8),
        WireKind::FixedArray { size } => format!("bytes_fixed_{size}"),
        WireKind::VarBytes => "bytes".into(),
        WireKind::Optional(inner) => wire_type_name(inner),
        WireKind::Struct(_) => "struct".into(),
        WireKind::VecOf(_) => "vector".into(),
        WireKind::Union(_) => "union".into(),
    }
}

/// Emit a `pub const _CKB_WITNESS_IDL_PATH: &str = "<path>";` token stream.
pub fn emit_const(idl_path: &Path) -> TokenStream {
    let path_str = idl_path.to_string_lossy();
    quote! {
        pub const _CKB_WITNESS_IDL_PATH: &str = #path_str;
    }
}

/// Emit static schema providers shared by the contract decoder and the host
/// exporter. Providers, rather than allocated recursive values, keep this
/// representation usable in `no_std` CKB contracts.
fn emit_schema_support(struct_name: &syn::Ident, fields: &[FieldMeta]) -> TokenStream {
    let provider_defs: Vec<TokenStream> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let provider = format_ident!("__ckb_idl_schema_{}_{}", struct_name, index);
            emit_type_provider(&provider, &field.wire_kind)
        })
        .collect();

    let schema_fields: Vec<TokenStream> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let provider = format_ident!("__ckb_idl_schema_{}_{}", struct_name, index);
            let name = &field.name;
            let required = field.required;
            let description = match &field.description {
                Some(value) => quote! { ::core::option::Option::Some(#value) },
                None => quote! { ::core::option::Option::None },
            };
            let semantic_type = match &field.type_override {
                Some(value) => quote! { ::core::option::Option::Some(#value) },
                None => quote! { ::core::option::Option::None },
            };
            quote! {
                ::ckb_idl_types::FieldSchema {
                    name: #name,
                    required: #required,
                    description: #description,
                    semantic_type: #semantic_type,
                    wire_type: #provider,
                }
            }
        })
        .collect();

    quote! {
        #(#provider_defs)*

        impl ::ckb_idl_types::WitnessSchema for #struct_name {
            fn schema() -> &'static ::ckb_idl_types::StructSchema {
                static FIELDS: &[::ckb_idl_types::FieldSchema] = &[
                    #(#schema_fields),*
                ];
                static SCHEMA: ::ckb_idl_types::StructSchema =
                    ::ckb_idl_types::StructSchema { fields: FIELDS };
                &SCHEMA
            }
        }
    }
}

fn emit_type_provider(name: &syn::Ident, wire_kind: &WireKind) -> TokenStream {
    match wire_kind {
        WireKind::FixedScalar { size } => {
            let bits = (*size as u16) * 8;
            quote! {
                #[allow(non_snake_case)]
                fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                    static TYPE: ::ckb_idl_types::TypeSchema =
                        ::ckb_idl_types::TypeSchema::Uint { bits: #bits };
                    &TYPE
                }
            }
        }
        WireKind::FixedArray { size } => quote! {
            #[allow(non_snake_case)]
            fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                static TYPE: ::ckb_idl_types::TypeSchema =
                    ::ckb_idl_types::TypeSchema::FixedBytes { length: #size };
                &TYPE
            }
        },
        WireKind::VarBytes => quote! {
            #[allow(non_snake_case)]
            fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                static TYPE: ::ckb_idl_types::TypeSchema = ::ckb_idl_types::TypeSchema::Bytes;
                &TYPE
            }
        },
        WireKind::Optional(inner) => {
            let inner_name = format_ident!("{}_inner", name);
            let inner_provider = emit_type_provider(&inner_name, inner);
            quote! {
                #inner_provider
                #[allow(non_snake_case)]
                fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                    static TYPE: ::ckb_idl_types::TypeSchema =
                        ::ckb_idl_types::TypeSchema::Optional { inner: #inner_name };
                    &TYPE
                }
            }
        }
        WireKind::VecOf(inner) => {
            let inner_name = format_ident!("{}_element", name);
            let inner_provider = emit_type_provider(&inner_name, inner);
            quote! {
                #inner_provider
                #[allow(non_snake_case)]
                fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                    static TYPE: ::ckb_idl_types::TypeSchema =
                        ::ckb_idl_types::TypeSchema::Vector {
                            element: #inner_name,
                            count_prefix_bits: 32,
                        };
                    &TYPE
                }
            }
        }
        WireKind::Struct(path) => quote! {
            #[allow(non_snake_case)]
            fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                static TYPE: ::ckb_idl_types::TypeSchema = ::ckb_idl_types::TypeSchema::Struct {
                    schema: <#path as ::ckb_idl_types::WitnessFields>::field_schema,
                };
                &TYPE
            }
        },
        WireKind::Union(path) => quote! {
            #[allow(non_snake_case)]
            fn #name() -> &'static ::ckb_idl_types::TypeSchema {
                static TYPE: ::ckb_idl_types::TypeSchema = ::ckb_idl_types::TypeSchema::Union {
                    schema: <#path as ::ckb_idl_types::WitnessUnion>::schema,
                };
                &TYPE
            }
        },
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

    let field_idents: Vec<_> = fields.iter().map(|f| &f.ident).collect();

    // Build the static FieldSpec slice entries.
    let field_specs: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let name_str = &f.name;
            let structural_type = wire_type_name(&f.wire_kind);
            let type_str = f.type_override.as_deref().unwrap_or(&structural_type);
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

    let schema_support = emit_schema_support(struct_name, fields);

    quote! {
        #schema_support

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

pub fn emit_union_impl(
    enum_name: &syn::Ident,
    variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>,
    tags: &[u32],
) -> TokenStream {
    let variant_specs: Vec<TokenStream> = variants
        .iter()
        .zip(tags.iter())
        .map(|(v, type_id)| {
            let variant_name_str = v.ident.to_string();
            // The inner type of the single-field tuple variant
            let inner_ty = match &v.fields {
                syn::Fields::Unnamed(f) => &f.unnamed[0].ty,
                _ => unreachable!("Validate ensures single unnamed field"),
            };
            quote! {
                ::ckb_idl_types::UnionVariantSpec {
                    type_id: #type_id,
                    name: #variant_name_str,
                    fields: <#inner_ty as ::ckb_idl_types::WitnessFields>::idl_fields,
                }
            }
        })
        .collect();

    // Build one decode arm per variant
    let decode_arms: Vec<TokenStream> = variants
        .iter()
        .zip(tags.iter())
        .map(|(v, type_id)| {
            let variant_ident = &v.ident;
            let inner_ty = match &v.fields {
                syn::Fields::Unnamed(f) => &f.unnamed[0].ty,
                _ => unreachable!(),
            };
            quote! {
                #type_id => {
                    let inner = <#inner_ty as ::ckb_idl_types::WitnessFields>
                        ::decode_fields(buf, cursor)?;
                    ::core::result::Result::Ok(#enum_name::#variant_ident(inner))
                }
            }
        })
        .collect();

    let enum_name_str = enum_name.to_string();

    let schema_variants: Vec<TokenStream> = variants
        .iter()
        .zip(tags.iter())
        .map(|(v, tag)| {
            let name = v.ident.to_string();
            let inner_ty = match &v.fields {
                syn::Fields::Unnamed(f) => &f.unnamed[0].ty,
                _ => unreachable!(),
            };
            quote! {
                ::ckb_idl_types::UnionVariantSchema {
                    tag: #tag,
                    name: #name,
                    schema: <#inner_ty as ::ckb_idl_types::WitnessFields>::field_schema,
                }
            }
        })
        .collect();

    quote! {
        impl ::ckb_idl_types::WitnessUnion for #enum_name {
            fn schema() -> &'static ::ckb_idl_types::UnionSchema {
                static VARIANTS: &[::ckb_idl_types::UnionVariantSchema] = &[
                    #(#schema_variants),*
                ];
                static SCHEMA: ::ckb_idl_types::UnionSchema =
                    ::ckb_idl_types::UnionSchema { variants: VARIANTS };
                &SCHEMA
            }

            fn idl_variants() -> &'static [::ckb_idl_types::UnionVariantSpec] {
                &[ #(#variant_specs),* ]
            }

            fn decode_union(
                buf: &[u8],
                cursor: &mut usize,
            ) -> ::core::result::Result<Self, ::ckb_idl_types::WitnessError> {
                let __tag_end = ::ckb_idl_types::checked_end(*cursor, 4, #enum_name_str)?;
                if __tag_end > buf.len() {
                    return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                        field: #enum_name_str,
                        expected: 4,
                        got: buf.len().saturating_sub(*cursor),
                    });
                }
                let __type_id = u32::from_le_bytes(
                    buf[*cursor..__tag_end].try_into().unwrap()
                );
                *cursor = __tag_end;
                match __type_id {
                    #(#decode_arms)*
                    _ => Err(::ckb_idl_types::WitnessError::UnknownUnionTypeId {
                        field: #enum_name_str,
                        type_id: __type_id,
                    }),
                }
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
            let ident = &f.ident;
            let name_str = &f.name;

            match &f.wire_kind {
                WireKind::FixedScalar { size: 1 } => quote! {
                    let #ident: u8 = {
                        let __end = ::ckb_idl_types::checked_end(cursor, 1, #name_str)?;
                        if __end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 1,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = buf[cursor];
                        cursor = __end;
                        v
                    };
                },

                WireKind::FixedScalar { size: 2 } => quote! {
                    let #ident: u16 = {
                        let __end = ::ckb_idl_types::checked_end(cursor, 2, #name_str)?;
                        if __end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 2,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u16::from_le_bytes(buf[cursor..__end].try_into().unwrap());
                        cursor = __end;
                        v
                    };
                },

                WireKind::FixedScalar { size: 4 } => quote! {
                    let #ident: u32 = {
                        let __end = ::ckb_idl_types::checked_end(cursor, 4, #name_str)?;
                        if __end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 4,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u32::from_le_bytes(buf[cursor..__end].try_into().unwrap());
                        cursor = __end;
                        v
                    };
                },

                WireKind::FixedScalar { size: 8 } => quote! {
                    let #ident: u64 = {
                        let __end = ::ckb_idl_types::checked_end(cursor, 8, #name_str)?;
                        if __end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 8,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u64::from_le_bytes(buf[cursor..__end].try_into().unwrap());
                        cursor = __end;
                        v
                    };
                },

                WireKind::FixedScalar { size: 16 } => quote! {
                    let #ident: u128 = {
                        let __end = ::ckb_idl_types::checked_end(cursor, 16, #name_str)?;
                        if __end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 16,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = u128::from_le_bytes(buf[cursor..__end].try_into().unwrap());
                        cursor = __end;
                        v
                    };
                },

                WireKind::FixedScalar { size: _ } => {
                    // map_wire_kind only emits size 1/2/4/8/16; anything else is a bug.
                    quote! { compile_error!("unsupported scalar size in CkbWitness codegen"); }
                }

                WireKind::FixedArray { size } => quote! {
                    let #ident: [u8; #size] = {
                        let __end = ::ckb_idl_types::checked_end(cursor, #size, #name_str)?;
                        if __end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: #size,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let mut arr = [0u8; #size];
                        arr.copy_from_slice(&buf[cursor..__end]);
                        cursor = __end;
                        arr
                    };
                },

                WireKind::VarBytes => quote! {
                    let #ident: ::alloc::vec::Vec<u8> = {
                        let __prefix_end = ::ckb_idl_types::checked_end(cursor, 4, #name_str)?;
                        if __prefix_end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: 4,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let len = u32::from_le_bytes(
                            buf[cursor..__prefix_end].try_into().unwrap()
                        ) as usize;
                        cursor = __prefix_end;
                        let __payload_end = ::ckb_idl_types::checked_end(cursor, len, #name_str)?;
                        if __payload_end > buf.len() {
                            return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                field: #name_str,
                                expected: len,
                                got: buf.len().saturating_sub(cursor),
                            });
                        }
                        let v = buf[cursor..__payload_end].to_vec();
                        cursor = __payload_end;
                        v
                    };
                },

                WireKind::VecOf(inner) => {
                    // For each inner kind, generate the element decode expression
                    // Produce a closure-like block that decodes one element and 
                    // returns it, then call it in a loop
                    let elem_decode = match inner.as_ref() {
                        WireKind::FixedScalar { size: 1 } => quote! {{
                            let __end = ::ckb_idl_types::checked_end(__cur, 1, #name_str)?;
                            if __end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::VecElementsTooShort {
                                    field: #name_str, element_index: __i,
                                });
                            }
                            let v = buf[__cur]; __cur = __end; v
                        }},
                        WireKind::FixedScalar { size: 2 } => quote! {{
                            let __end = ::ckb_idl_types::checked_end(__cur, 2, #name_str)?;
                            if __end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::VecElementsTooShort {
                                    field: #name_str, element_index: __i,
                                });
                            }
                            let v = u16::from_le_bytes(buf[__cur..__end].try_into().unwrap()); __cur = __end; v
                        }},
                        WireKind::FixedScalar { size: 4 } => quote! {{
                            let __end = ::ckb_idl_types::checked_end(__cur, 4, #name_str)?;
                            if __end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::VecElementsTooShort {
                                    field: #name_str, element_index: __i,
                                });
                            }
                            let v = u32::from_le_bytes(buf[__cur..__end].try_into().unwrap()); __cur = __end; v
                        }},
                        WireKind::FixedScalar { size: 8 } => quote! {{
                            let __end = ::ckb_idl_types::checked_end(__cur, 8, #name_str)?;
                            if __end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::VecElementsTooShort {
                                    field: #name_str, element_index: __i,
                                });
                            }
                            let v = u64::from_le_bytes(buf[__cur..__end].try_into().unwrap()); __cur = __end; v
                        }},
                        WireKind::FixedScalar { size: 16 } => quote! {{
                            let __end = ::ckb_idl_types::checked_end(__cur, 16, #name_str)?;
                            if __end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::VecElementsTooShort {
                                    field: #name_str, element_index: __i,
                                });
                            }
                            let v = u128::from_le_bytes(buf[__cur..__end].try_into().unwrap()); __cur = __end; v
                        }},
                        WireKind::FixedArray { size } => quote! {{
                            let __end = ::ckb_idl_types::checked_end(__cur, #size, #name_str)?;
                            if __end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::VecElementsTooShort {
                                    field: #name_str, element_index: __i,
                                });
                            }
                            let mut arr = [0u8; #size];
                            arr.copy_from_slice(&buf[__cur..__end]);
                            __cur = __end;
                            arr
                        }},
                        WireKind::Struct(type_path) => quote! {
                            <#type_path as ::ckb_idl_types::WitnessFields>::decode_fields(buf, &mut __cur)?
                        },
                        _ => quote! { compile_error!("unsupported VecOf inner kind in CkbWitness codegen"); },
                    };
                    quote! {
                        let #ident = {
                            let __prefix_end = ::ckb_idl_types::checked_end(cursor, 4, #name_str)?;
                            if __prefix_end > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 4,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            }
                            let __count = u32::from_le_bytes(
                                buf[cursor..__prefix_end].try_into().unwrap()
                            ) as usize;
                            cursor = __prefix_end;
                            if __count == 0 {
                                return Err(::ckb_idl_types::WitnessError::InvalidVectorCount {
                                    field: #name_str,
                                    count: 0,
                                });
                            }
                            let mut __cur = cursor;
                            // Cap the initial allocation to the remaining buffer
                            // length so a malformed count cannot cause an
                            // oversized allocation before any bytes are read.
                            let mut __vec = ::alloc::vec::Vec::with_capacity(
                                __count.min(buf.len().saturating_sub(__cur))
                            );
                            for __i in 0..__count {
                                __vec.push(#elem_decode);
                            }
                            cursor = __cur;
                            __vec
                        };
                    }
                },

                WireKind::Optional(inner) => match inner.as_ref() {
                    WireKind::VarBytes => quote! {
                        let #ident: ::core::option::Option<::alloc::vec::Vec<u8>> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else {
                                let __prefix_end = ::ckb_idl_types::checked_end(cursor, 4, #name_str)?;
                                if __prefix_end > buf.len() {
                                    return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                        field: #name_str,
                                        expected: 4,
                                        got: buf.len().saturating_sub(cursor),
                                    });
                                }
                                let len = u32::from_le_bytes(
                                    buf[cursor..__prefix_end].try_into().unwrap()
                                ) as usize;
                                cursor = __prefix_end;
                                let __payload_end = ::ckb_idl_types::checked_end(cursor, len, #name_str)?;
                                if __payload_end > buf.len() {
                                    return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                        field: #name_str,
                                        expected: len,
                                        got: buf.len().saturating_sub(cursor),
                                    });
                                }
                                let v = buf[cursor..__payload_end].to_vec();
                                cursor = __payload_end;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedArray { size } => quote! {
                        let #ident: ::core::option::Option<[u8; #size]> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else {
                                let __end = ::ckb_idl_types::checked_end(cursor, #size, #name_str)?;
                                if __end > buf.len() {
                                    return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                        field: #name_str,
                                        expected: #size,
                                        got: buf.len().saturating_sub(cursor),
                                    });
                                }
                                let mut arr = [0u8; #size];
                                arr.copy_from_slice(&buf[cursor..__end]);
                                cursor = __end;
                                ::core::option::Option::Some(arr)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 1 } => quote! {
                        let #ident: ::core::option::Option<u8> = {
                            if cursor >= buf.len() { ::core::option::Option::None }
                            else {
                                let __end = ::ckb_idl_types::checked_end(cursor, 1, #name_str)?;
                                let value = buf[cursor]; cursor = __end;
                                ::core::option::Option::Some(value)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 2 } => quote! {
                        let #ident: ::core::option::Option<u16> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if ::ckb_idl_types::checked_end(cursor, 2, #name_str)? > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 2,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u16::from_le_bytes(buf[cursor..::ckb_idl_types::checked_end(cursor, 2, #name_str)?].try_into().unwrap());
                                cursor = ::ckb_idl_types::checked_end(cursor, 2, #name_str)?;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 4 } => quote! {
                        let #ident: ::core::option::Option<u32> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if ::ckb_idl_types::checked_end(cursor, 4, #name_str)? > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 4,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u32::from_le_bytes(buf[cursor..::ckb_idl_types::checked_end(cursor, 4, #name_str)?].try_into().unwrap());
                                cursor = ::ckb_idl_types::checked_end(cursor, 4, #name_str)?;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 8 } => quote! {
                        let #ident: ::core::option::Option<u64> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if ::ckb_idl_types::checked_end(cursor, 8, #name_str)? > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 8,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u64::from_le_bytes(buf[cursor..::ckb_idl_types::checked_end(cursor, 8, #name_str)?].try_into().unwrap());
                                cursor = ::ckb_idl_types::checked_end(cursor, 8, #name_str)?;
                                ::core::option::Option::Some(v)
                            }
                        };
                    },
                    WireKind::FixedScalar { size: 16 } => quote! {
                        let #ident: ::core::option::Option<u128> = {
                            if cursor >= buf.len() {
                                ::core::option::Option::None
                            } else if ::ckb_idl_types::checked_end(cursor, 16, #name_str)? > buf.len() {
                                return Err(::ckb_idl_types::WitnessError::FieldTooShort {
                                    field: #name_str,
                                    expected: 16,
                                    got: buf.len().saturating_sub(cursor),
                                });
                            } else {
                                let v = u128::from_le_bytes(buf[cursor..::ckb_idl_types::checked_end(cursor, 16, #name_str)?].try_into().unwrap());
                                cursor = ::ckb_idl_types::checked_end(cursor, 16, #name_str)?;
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

                WireKind::Union(type_path) => quote! {
                    let #ident = <#type_path as ::ckb_idl_types::WitnessUnion>::decode_union(
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
    let schema_support = emit_schema_support(struct_name, fields);

    // Identifiers for the struct construction expression.
    let field_idents: Vec<_> = fields.iter().map(|f| &f.ident).collect();

    quote! {
        #schema_support

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
        _idl_type: &str,
        required: bool,
        description: Option<&str>,
        wire_kind: WireKind,
    ) -> FieldMeta {
        FieldMeta {
            ident: format_ident!("{name}"),
            name: name.to_string(),
            required,
            description: description.map(|s| s.to_string()),
            type_override: None,
            wire_kind,
        }
    }

    fn make_field_with_override(
        name: &str,
        _idl_type: &str,
        type_override: &str,
        wire_kind: WireKind,
    ) -> FieldMeta {
        FieldMeta {
            ident: format_ident!("{name}"),
            name: name.to_string(),
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
        assert!(idl["interfaces"][0]["fields"].is_array());
    }

    #[test]
    fn top_level_idl_version_is_point_one() {
        let idl = build_idl(&[]);
        assert_eq!(idl["idl_version"].as_str().unwrap(), "0.1.0");
    }

    #[test]
    fn top_level_encoding_block_is_present() {
        let idl = build_idl(&[]);
        assert_eq!(
            idl["interfaces"][0]["encoding"]["id"],
            "ckb-idl-linear-0.1.0"
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
        assert_eq!(
            build_idl(&fields)["interfaces"][0]["fields"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
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
        let arr = build_idl(&fields)["interfaces"][0]["fields"]
            .as_array()
            .unwrap()
            .clone();
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
        assert!(
            idl["interfaces"][0]["fields"][0]
                .get("description")
                .is_none()
        );
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
        assert_eq!(idl["interfaces"][0]["fields"][0]["description"], "my desc");
    }

    #[test]
    fn empty_fields_produces_empty_array() {
        assert_eq!(
            build_idl(&[])["interfaces"][0]["fields"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn typed_vector_uses_structured_items() {
        let idl = build_idl(&[make_field(
            "values",
            "vec_of_uint64",
            true,
            None,
            WireKind::VecOf(Box::new(WireKind::FixedScalar { size: 8 })),
        )]);
        let field = &idl["interfaces"][0]["fields"][0];
        assert_eq!(field["type"], "vector");
        assert_eq!(field["items"]["type"], "uint64");
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
        assert_eq!(idl["interfaces"][0]["fields"][0]["type"], "secp256k1_sig");
        assert_eq!(
            idl["interfaces"][0]["fields"][0]["wire_type"],
            "bytes_fixed_65"
        );
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
        assert_eq!(idl["interfaces"][0]["fields"][0]["type"], "bytes_fixed_65");
    }

    #[test]
    fn type_override_arbitrary_label() {
        let idl = build_idl(&[make_field_with_override(
            "proof",
            "bytes_fixed_32",
            "my-project:custom_proof",
            WireKind::FixedArray { size: 32 },
        )]);
        assert_eq!(
            idl["interfaces"][0]["fields"][0]["type"],
            "my-project:custom_proof"
        );
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
                "my-project:bls12_381_half",
                WireKind::FixedArray { size: 64 },
            ),
        ];
        let idl = build_idl(&fields);
        assert_eq!(idl["interfaces"][0]["fields"][0]["type"], "schnorr_sig");
        assert_eq!(
            idl["interfaces"][0]["fields"][1]["type"],
            "my-project:bls12_381_half"
        );
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
            "[a-z][a-z0-9_]{0,15}".prop_filter(
                "field names must also be valid non-keyword Rust identifiers",
                |name| syn::parse_str::<syn::Ident>(name).is_ok(),
            ),
            arb_idl_type(),
            any::<bool>(),
            proptest::option::of("[^\x00]{1,64}"),
            proptest::option::of("[a-z][a-z0-9-]{0,8}:[a-z][a-z0-9_]{0,12}"),
        )
            .prop_map(
                |(name, (_idl_type, wire_kind), required, description, type_override)| FieldMeta {
                    ident: syn::parse_str(&name).expect("generated field name must be valid"),
                    name,
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
            prop_assert_eq!(idl["idl_version"].as_str().unwrap(), "0.1.0");
            prop_assert_eq!(idl["interfaces"][0]["encoding"]["id"].as_str().unwrap(), "ckb-idl-linear-0.1.0");

            let arr = idl["interfaces"][0]["fields"].as_array()
                .expect("interface fields must be a JSON array");

            prop_assert_eq!(arr.len(), n);

            for (i, (elem, field)) in arr.iter().zip(fields.iter()).enumerate() {
                prop_assert!(elem["name"].is_string(),      "element {i} missing string \"name\"");
                prop_assert!(elem["type"].is_string(),      "element {i} missing string \"type\"");
                prop_assert!(elem["required"].is_boolean(), "element {i} missing boolean \"required\"");
                prop_assert_eq!(elem["name"].as_str().unwrap(), field.name.as_str());

                // type must be the override when present, structural otherwise
                let structural_type = wire_type_name(&field.wire_kind);
                let expected_type = field.type_override.as_deref().unwrap_or(&structural_type);
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
                    ident: format_ident!("field_{i}"),
                    name: format!("field_{i}"),
                    required: *req,
                    description: desc.clone(),
                    type_override: None,
                    wire_kind: WireKind::FixedScalar { size: 1 },
                })
                .collect();

            let idl = build_idl(&fields);
            let arr = idl["interfaces"][0]["fields"].as_array().unwrap();

            for (elem, (req, _)) in arr.iter().zip(inputs.iter()) {
                prop_assert_eq!(elem["required"].as_bool().unwrap(), *req);
            }
        }

        #[test]
        fn prop2_description_round_trip(desc in "[^\x00]{1,128}") {
            let fields = vec![FieldMeta {
                ident: format_ident!("x"),
                name: "x".to_string(),
                required: true,
                description: Some(desc.clone()),
                type_override: None,
                wire_kind: WireKind::FixedScalar { size: 1 },
            }];

            let got = build_idl(&fields)["interfaces"][0]["fields"][0]["description"]
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
