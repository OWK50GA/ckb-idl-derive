use syn::{Expr, ExprLit, GenericArgument, Lit, PathArguments, Type};

/// The wire kind of a field — used by codegen to emit the correct
/// deserialization snippet for `from_witness_args`.
#[derive(Clone)]
pub enum WireKind {
    /// Fixed-size copy type: u8, u16, u32, u64, u128. `size` is the byte width.
    FixedScalar { size: usize },
    /// Fixed-size byte array: [u8; N]. `size` is N.
    FixedArray { size: usize },
    /// Variable-length byte sequence: Vec<u8>. Length-prefixed on the wire.
    VarBytes,
    /// Optional field: Option<T>. The inner WireKind describes T.
    /// Decodes as None when the remaining buffer is exhausted, Some(T) otherwise.
    Optional(Box<WireKind>),
    /// A nested struct implementing `WitnessFields`.
    /// Decoded by delegating to `<T as WitnessFields>::decode_fields`.
    /// The type path is stored so codegen can emit the correct trait call.
    Struct(syn::Path),
    /// Variable-count sequence of typed elements: Vec<T> where T is not u8.
    /// Wire format: 4-byte LE u32 element count, followed by N encoded elements
    VecOf(Box<WireKind>),
    /// A field whose type implements `WitnessUnion`.
    /// Decoded by dispatching on a 4-byte LE type ID tag via `decode_union`.
    /// Signalled by `#[witness(union)]` on the field.
    Union(syn::Path),
}

impl core::fmt::Debug for WireKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FixedScalar { size } => write!(f, "FixedScalar {{ size: {size} }}"),
            Self::FixedArray { size } => write!(f, "FixedArray {{ size: {size} }}"),
            Self::VarBytes => write!(f, "VarBytes"),
            Self::Optional(inner) => write!(f, "Optional({inner:?})"),
            Self::Struct(_) => write!(f, "Struct(..)"),
            Self::VecOf(inner) => write!(f, "VecOf({inner:?})"),
            Self::Union(_) => write!(f, "Union(..)"),
        }
    }
}

impl PartialEq for WireKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::FixedScalar { size: a }, Self::FixedScalar { size: b }) => a == b,
            (Self::FixedArray { size: a }, Self::FixedArray { size: b }) => a == b,
            (Self::VarBytes, Self::VarBytes) => true,
            (Self::Optional(a), Self::Optional(b)) => a == b,
            (Self::Struct(a), Self::Struct(b)) | (Self::Union(a), Self::Union(b)) => {
                quote::quote!(#a).to_string() == quote::quote!(#b).to_string()
            }
            (Self::VecOf(a), Self::VecOf(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for WireKind {}

/// Maps a Rust `syn::Type` to a structural IDL type string.
///
/// All `[u8; N]` arrays map to `"bytes_fixed_N"` (e.g. `"bytes_fixed_32"`).
/// Semantic labels (e.g. `"secp256k1_sig"`) are supplied separately via
/// `#[witness(type = "...")]` and never forced by the registry.
///
/// `field_name` is used only for the error message when the type is unrecognised.
pub fn map_type(ty: &Type, field_name: &str) -> syn::Result<String> {
    match ty {
        // Handle Type::Path: u8, u16, u32, u64, u128, Vec<u8>
        Type::Path(type_path) => {
            let segments = &type_path.path.segments;

            // Check for Vec<u8>
            if let Some(last) = segments.last() {
                if last.ident == "Vec" {
                    if let PathArguments::AngleBracketed(ref args) = last.arguments
                        && args.args.len() == 1
                        && let Some(GenericArgument::Type(Type::Path(inner))) = args.args.first()
                        && inner.path.is_ident("u8")
                    {
                        return Ok("bytes".to_string());
                    }
                } else if last.ident == "Option" {
                    // Option<T> — delegate to the inner type.
                    // The IDL type string is identical to T's; the Optional
                    // wrapper is carried in WireKind, not in the type string.
                    //
                    // Nested Option<Option<T>> is rejected: the wire format
                    // has no encoding for a doubly-optional field, and the
                    // codegen has no Optional(Optional(...)) arm.
                    if let PathArguments::AngleBracketed(ref args) = last.arguments
                        && args.args.len() == 1
                        && let Some(GenericArgument::Type(inner_ty)) = args.args.first()
                    {
                        // Reject Option<Option<T>> before recursing.
                        if let Type::Path(inner_path) = inner_ty
                            && inner_path
                                .path
                                .segments
                                .last()
                                .map(|s| s.ident == "Option")
                                .unwrap_or(false)
                        {
                            return Err(syn::Error::new_spanned(
                                ty,
                                format!(
                                    "nested `Option<Option<T>>` is not supported for field \
                                         `{field_name}`; use `Option<T>` directly"
                                ),
                            ));
                        }

                        return map_type(inner_ty, field_name);
                    }
                }
                // Vec<T> where T is not u8 — check this after Vec<u8>
                if let Some(last) = segments.last()
                    && last.ident == "Vec"
                    && let PathArguments::AngleBracketed(ref args) = last.arguments
                    && args.args.len() == 1
                    && let Some(GenericArgument::Type(inner_ty)) = args.args.first()
                {
                    // Reject element types that have no supported VecOf decoder arm.
                    // Supported: scalars, [u8; N], named structs (WitnessFields).
                    // Unsupported: Vec<Vec<T>>, Vec<Option<T>>, Vec<[non-u8; N]>.
                    let reject = match inner_ty {
                        // Vec<Vec<T>> — nested vectors are not supported.
                        Type::Path(p)
                            if p.path
                                .segments
                                .last()
                                .map(|s| s.ident == "Vec")
                                .unwrap_or(false) =>
                        {
                            Some(
                                "Vec<Vec<T>> is not supported as a field type; \
                                  use a named struct with a Vec<u8> field instead",
                            )
                        }
                        // Vec<Option<T>> — optional elements are not supported.
                        Type::Path(p)
                            if p.path
                                .segments
                                .last()
                                .map(|s| s.ident == "Option")
                                .unwrap_or(false) =>
                        {
                            Some(
                                "Vec<Option<T>> is not supported as a field type; \
                                  use a required inner type",
                            )
                        }
                        _ => None,
                    };
                    if let Some(msg) = reject {
                        return Err(syn::Error::new_spanned(
                            inner_ty,
                            format!("unsupported element type for field `{field_name}`: {msg}"),
                        ));
                    }
                    // Vec<T> where T is a named struct — rejected because we
                    // cannot know at macro expansion time whether the inner struct
                    // has optional trailing fields, making the element boundary
                    // unsafe without a per-element length prefix.
                    // Structs in Vec context would need a separate framing scheme.
                    // Reject at map_type so the codegen never sees VecOf(Struct).
                    let inner_kind = map_wire_kind(inner_ty);
                    if matches!(inner_kind, Some(crate::registry::WireKind::Struct(_))) {
                        return Err(syn::Error::new_spanned(
                            inner_ty,
                            format!(
                                "unsupported element type for field `{field_name}`: \
                                 Vec<NamedStruct> is not supported; nested structs in a Vec \
                                 have no safe element boundary under the current wire format"
                            ),
                        ));
                    }
                    let inner_idl = map_type(inner_ty, field_name)?;
                    return Ok(format!("vec_of_{inner_idl}"));
                }
            }

            // Check for single-segment primitives
            if segments.len() == 1 {
                let ident = &segments[0].ident;
                match ident.to_string().as_str() {
                    "u8" => return Ok("uint8".to_string()),
                    "u16" => return Ok("uint16".to_string()),
                    "u32" => return Ok("uint32".to_string()),
                    "u64" => return Ok("uint64".to_string()),
                    "u128" => return Ok("uint128".to_string()),
                    _ => {}
                }
            }

            // Any other Type::Path is treated as a nested struct that must
            // implement WitnessFields. The compiler will enforce the trait
            // bound when the generated decode call is compiled.
            Ok("struct".to_string())
        }

        // Handle Type::Array: any [u8; N] → "bytes_fixed_N".
        // Semantic meaning (e.g. "secp256k1_sig", "blake2b_hash") is the
        // user's responsibility and is expressed via #[witness(type = "...")].
        Type::Array(type_array) => {
            let elem_is_u8 = match type_array.elem.as_ref() {
                Type::Path(p) => p.path.is_ident("u8"),
                _ => false,
            };

            if elem_is_u8
                && let Expr::Lit(ExprLit {
                    lit: Lit::Int(n), ..
                }) = &type_array.len
                && let Ok(size) = n.base10_parse::<usize>()
            {
                if size == 0 {
                    return Err(syn::Error::new_spanned(
                        type_array,
                        format!(
                            "zero-length byte arrays `[u8; 0]` are not supported for field \
                             `{field_name}`; use `Vec<u8>` for variable-length bytes"
                        ),
                    ));
                }
                return Ok(format!("bytes_fixed_{size}"));
            }

            Err(make_error(ty, field_name))
        }

        _ => Err(make_error(ty, field_name)),
    }
}

/// Maps a Rust `syn::Type` to a `WireKind`, which drives deserialization
/// code generation in `codegen::emit_impl`.
///
/// Returns `None` for unrecognised types (caller will have already errored
/// via `map_type`, so this should never be reached in practice).
pub fn map_wire_kind(ty: &Type) -> Option<WireKind> {
    match ty {
        Type::Path(type_path) => {
            let segments = &type_path.path.segments;

            // Vec<u8> → VarBytes
            if let Some(last) = segments.last()
                && last.ident == "Vec"
                && let PathArguments::AngleBracketed(ref args) = last.arguments
                && args.args.len() == 1
                && let Some(GenericArgument::Type(Type::Path(inner))) = args.args.first()
                && inner.path.is_ident("u8")
            {
                return Some(WireKind::VarBytes);
            }

            // Vec<T> where T is not u8 → VecOf(inner WireKind)
            if let Some(last) = segments.last()
                && last.ident == "Vec"
                && let PathArguments::AngleBracketed(ref args) = last.arguments
                && args.args.len() == 1
                && let Some(GenericArgument::Type(inner_ty)) = args.args.first()
            {
                return map_wire_kind(inner_ty).map(|k| WireKind::VecOf(Box::new(k)));
            }

            // Option<T> → Optional(inner WireKind)
            if let Some(last) = segments.last()
                && last.ident == "Option"
                && let PathArguments::AngleBracketed(ref args) = last.arguments
                && args.args.len() == 1
                && let Some(GenericArgument::Type(inner_ty)) = args.args.first()
            {
                return map_wire_kind(inner_ty).map(|k| WireKind::Optional(Box::new(k)));
            }

            // Scalar primitives
            if segments.len() == 1 {
                match segments[0].ident.to_string().as_str() {
                    "u8" => return Some(WireKind::FixedScalar { size: 1 }),
                    "u16" => return Some(WireKind::FixedScalar { size: 2 }),
                    "u32" => return Some(WireKind::FixedScalar { size: 4 }),
                    "u64" => return Some(WireKind::FixedScalar { size: 8 }),
                    "u128" => return Some(WireKind::FixedScalar { size: 16 }),
                    _ => {}
                }
            }

            // Any other path is a nested struct implementing WitnessFields.
            Some(WireKind::Struct(type_path.path.clone()))
        }

        // [u8; N] → FixedArray { size: N }
        Type::Array(type_array) => {
            let elem_is_u8 = match type_array.elem.as_ref() {
                Type::Path(p) => p.path.is_ident("u8"),
                _ => false,
            };

            if elem_is_u8
                && let Expr::Lit(ExprLit {
                    lit: Lit::Int(n), ..
                }) = &type_array.len
                && let Ok(size) = n.base10_parse::<usize>()
                && size > 0
            {
                return Some(WireKind::FixedArray { size });
            }

            None
        }

        _ => None,
    }
}

fn make_error(ty: &Type, field_name: &str) -> syn::Error {
    let type_str = quote::quote!(#ty).to_string();
    let msg = format!(
        "unrecognised type `{type_str}` for field `{field_name}`; \
         supported types are: u8, u16, u32, u64, u128, [u8; N] for any N, Vec<u8>, Option<T>, \
         or any named struct implementing WitnessFields"
    );
    syn::Error::new_spanned(ty, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;

    fn parse(s: &str) -> Type {
        parse_str::<Type>(s).expect("failed to parse type")
    }

    // ── Scalar happy-path tests ───────────────────────────────────────────────

    #[test]
    fn test_u8_maps_to_uint8() {
        assert_eq!(map_type(&parse("u8"), "f").unwrap(), "uint8");
    }

    #[test]
    fn test_u16_maps_to_uint16() {
        assert_eq!(map_type(&parse("u16"), "f").unwrap(), "uint16");
    }

    #[test]
    fn test_u32_maps_to_uint32() {
        assert_eq!(map_type(&parse("u32"), "f").unwrap(), "uint32");
    }

    #[test]
    fn test_u64_maps_to_uint64() {
        assert_eq!(map_type(&parse("u64"), "f").unwrap(), "uint64");
    }

    #[test]
    fn test_u128_maps_to_uint128() {
        assert_eq!(map_type(&parse("u128"), "f").unwrap(), "uint128");
    }

    #[test]
    fn test_vec_u8_maps_to_bytes() {
        assert_eq!(map_type(&parse("Vec<u8>"), "f").unwrap(), "bytes");
    }

    // ── Array happy-path tests ────────────────────────────────────────────────
    // All [u8; N] map to "bytes_fixed_N" regardless of N.
    // The user adds #[witness(type = "secp256k1_sig")] etc. for semantic labels.

    #[test]
    fn test_array_32_maps_to_bytes_fixed_32() {
        assert_eq!(map_type(&parse("[u8; 32]"), "f").unwrap(), "bytes_fixed_32");
    }

    #[test]
    fn test_array_33_maps_to_bytes_fixed_33() {
        assert_eq!(map_type(&parse("[u8; 33]"), "f").unwrap(), "bytes_fixed_33");
    }

    #[test]
    fn test_array_64_maps_to_bytes_fixed_64() {
        assert_eq!(map_type(&parse("[u8; 64]"), "f").unwrap(), "bytes_fixed_64");
    }

    #[test]
    fn test_array_65_maps_to_bytes_fixed_65() {
        assert_eq!(map_type(&parse("[u8; 65]"), "f").unwrap(), "bytes_fixed_65");
    }

    #[test]
    fn test_array_1_maps_to_bytes_fixed_1() {
        assert_eq!(map_type(&parse("[u8; 1]"), "f").unwrap(), "bytes_fixed_1");
    }

    #[test]
    fn test_zero_length_array_is_rejected() {
        let err = map_type(&parse("[u8; 0]"), "empty").unwrap_err();
        assert!(err.to_string().contains("zero-length byte arrays"));
        assert_eq!(map_wire_kind(&parse("[u8; 0]")), None);
    }

    #[test]
    fn test_array_96_maps_to_bytes_fixed_96() {
        assert_eq!(map_type(&parse("[u8; 96]"), "f").unwrap(), "bytes_fixed_96");
    }

    // ── Error-path test ───────────────────────────────────────────────────────
    // Note: Type::Path types that are not known primitives now map to "struct"
    // rather than erroring. The only types that still error at map_type time are
    // non-u8 arrays and other non-Path, non-Array types.

    #[test]
    fn test_string_maps_to_struct() {
        // String is a Type::Path — it now succeeds as a struct type.
        // The compiler enforces WitnessFields at codegen time, not here.
        assert_eq!(map_type(&parse("String"), "my_field").unwrap(), "struct");
    }

    #[test]
    fn test_error_message_on_non_u8_array() {
        // [u32; 4] has a non-u8 element — this still errors at map_type time.
        let err = map_type(&parse("[u32; 4]"), "f").unwrap_err();
        assert!(err.to_string().contains("unrecognised type"), "{}", err);
    }

    // ── WireKind tests ────────────────────────────────────────────────────────

    #[test]
    fn wire_kind_u8() {
        assert_eq!(
            map_wire_kind(&parse("u8")),
            Some(WireKind::FixedScalar { size: 1 })
        );
    }

    #[test]
    fn wire_kind_u16() {
        assert_eq!(
            map_wire_kind(&parse("u16")),
            Some(WireKind::FixedScalar { size: 2 })
        );
    }

    #[test]
    fn wire_kind_u32() {
        assert_eq!(
            map_wire_kind(&parse("u32")),
            Some(WireKind::FixedScalar { size: 4 })
        );
    }

    #[test]
    fn wire_kind_u64() {
        assert_eq!(
            map_wire_kind(&parse("u64")),
            Some(WireKind::FixedScalar { size: 8 })
        );
    }

    #[test]
    fn wire_kind_u128() {
        assert_eq!(
            map_wire_kind(&parse("u128")),
            Some(WireKind::FixedScalar { size: 16 })
        );
    }

    #[test]
    fn wire_kind_array_any_n() {
        assert_eq!(
            map_wire_kind(&parse("[u8; 32]")),
            Some(WireKind::FixedArray { size: 32 })
        );
        assert_eq!(
            map_wire_kind(&parse("[u8; 33]")),
            Some(WireKind::FixedArray { size: 33 })
        );
        assert_eq!(
            map_wire_kind(&parse("[u8; 64]")),
            Some(WireKind::FixedArray { size: 64 })
        );
        assert_eq!(
            map_wire_kind(&parse("[u8; 65]")),
            Some(WireKind::FixedArray { size: 65 })
        );
        assert_eq!(
            map_wire_kind(&parse("[u8; 96]")),
            Some(WireKind::FixedArray { size: 96 })
        );
        assert_eq!(
            map_wire_kind(&parse("[u8; 128]")),
            Some(WireKind::FixedArray { size: 128 })
        );
    }

    #[test]
    fn path_wire_kinds_are_reflexive_and_distinguish_paths() {
        let auth = map_wire_kind(&parse("AuthMethod")).unwrap();
        let other = map_wire_kind(&parse("OtherMethod")).unwrap();
        assert_eq!(auth, auth.clone());
        assert_ne!(auth, other);

        let union_auth = WireKind::Union(match parse("AuthMethod") {
            Type::Path(path) => path.path,
            _ => unreachable!(),
        });
        let union_other = WireKind::Union(match parse("OtherMethod") {
            Type::Path(path) => path.path,
            _ => unreachable!(),
        });
        assert_eq!(union_auth, union_auth.clone());
        assert_ne!(union_auth, union_other);
    }

    #[test]
    fn wire_kind_vec_u8() {
        assert_eq!(map_wire_kind(&parse("Vec<u8>")), Some(WireKind::VarBytes));
    }

    // ── Option<T> tests ───────────────────────────────────────────────────────

    #[test]
    fn option_vec_u8_maps_to_bytes_idl_type() {
        // IDL type is the same as the inner type
        assert_eq!(map_type(&parse("Option<Vec<u8>>"), "f").unwrap(), "bytes");
    }

    #[test]
    fn option_array_maps_to_bytes_fixed_idl_type() {
        assert_eq!(
            map_type(&parse("Option<[u8; 32]>"), "f").unwrap(),
            "bytes_fixed_32"
        );
    }

    #[test]
    fn option_u64_maps_to_uint64_idl_type() {
        assert_eq!(map_type(&parse("Option<u64>"), "f").unwrap(), "uint64");
    }

    #[test]
    fn option_vec_u8_wire_kind() {
        assert_eq!(
            map_wire_kind(&parse("Option<Vec<u8>>")),
            Some(WireKind::Optional(Box::new(WireKind::VarBytes)))
        );
    }

    #[test]
    fn option_array_wire_kind() {
        assert_eq!(
            map_wire_kind(&parse("Option<[u8; 32]>")),
            Some(WireKind::Optional(Box::new(WireKind::FixedArray {
                size: 32
            })))
        );
    }

    #[test]
    fn option_u64_wire_kind() {
        assert_eq!(
            map_wire_kind(&parse("Option<u64>")),
            Some(WireKind::Optional(Box::new(WireKind::FixedScalar {
                size: 8
            })))
        );
    }

    #[test]
    fn option_u8_wire_kind() {
        assert_eq!(
            map_wire_kind(&parse("Option<u8>")),
            Some(WireKind::Optional(Box::new(WireKind::FixedScalar {
                size: 1
            })))
        );
    }

    // ── Vec<T> tests ───────────────────────────────────────────────────────
    #[test]
    fn vec_u64_maps_to_vec_of_uint64() {
        assert_eq!(map_type(&parse("Vec<u64>"), "f").unwrap(), "vec_of_uint64");
    }

    #[test]
    fn vec_array_maps_to_vec_of_bytes_fixed() {
        assert_eq!(
            map_type(&parse("Vec<[u8; 33]>"), "f").unwrap(),
            "vec_of_bytes_fixed_33"
        );
    }

    #[test]
    fn vec_u64_wire_kind() {
        assert_eq!(
            map_wire_kind(&parse("Vec<u64>")),
            Some(WireKind::VecOf(Box::new(WireKind::FixedScalar { size: 8 })))
        )
    }

    #[test]
    fn vec_array_wire_kind() {
        assert_eq!(
            map_wire_kind(&parse("Vec<[u8; 33]>")),
            Some(WireKind::VecOf(Box::new(WireKind::FixedArray { size: 33 })))
        )
    }
}
