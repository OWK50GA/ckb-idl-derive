use syn::{Expr, ExprLit, GenericArgument, Lit, PathArguments, Type};

/// The wire kind of a field — used by codegen to emit the correct
/// deserialization snippet for `from_witness_args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    /// Fixed-size copy type: u8, u16, u32, u64, u128. `size` is the byte width.
    FixedScalar { size: usize },
    /// Fixed-size byte array: [u8; N]. `size` is N.
    FixedArray { size: usize },
    /// Variable-length byte sequence: Vec<u8>. Length-prefixed on the wire.
    VarBytes,
}

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
            if let Some(last) = segments.last()
                && last.ident == "Vec"
                && let PathArguments::AngleBracketed(ref args) = last.arguments
                && args.args.len() == 1
                && let Some(GenericArgument::Type(Type::Path(inner))) = args.args.first()
                && inner.path.is_ident("u8")
            {
                return Ok("bytes".to_string());
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

            Err(make_error(ty, field_name))
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

            None
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
         supported types are: u8, u16, u32, u64, u128, [u8; N] for any N, Vec<u8>"
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
    fn test_array_96_maps_to_bytes_fixed_96() {
        assert_eq!(map_type(&parse("[u8; 96]"), "f").unwrap(), "bytes_fixed_96");
    }

    // ── Error-path test ───────────────────────────────────────────────────────

    #[test]
    fn test_string_produces_error() {
        let err = map_type(&parse("String"), "my_field").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unrecognised type"),
            "missing 'unrecognised type': {msg}"
        );
        assert!(msg.contains("String"), "missing type name: {msg}");
        assert!(msg.contains("my_field"), "missing field name: {msg}");
    }

    #[test]
    fn test_error_message_mentions_array_syntax() {
        let err = map_type(&parse("String"), "f").unwrap_err();
        assert!(
            err.to_string().contains("[u8; N]"),
            "should mention [u8; N]: {}",
            err
        );
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
    fn wire_kind_vec_u8() {
        assert_eq!(map_wire_kind(&parse("Vec<u8>")), Some(WireKind::VarBytes));
    }
}
