use syn::{Expr, ExprLit, GenericArgument, Lit, PathArguments, Type};

/// The wire kind of a field — used by codegen to emit the correct
/// deserialization snippet for `from_witness_args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    /// Fixed-size copy type: u8, u32, u64. `size` is the byte width.
    FixedScalar { size: usize },
    /// Fixed-size byte array: [u8; N]. `size` is N.
    FixedArray { size: usize },
    /// Variable-length byte sequence: Vec<u8>. Length-prefixed on the wire.
    VarBytes,
}

/// Maps a Rust `syn::Type` to a blessed IDL type string.
///
/// `field_name` is used only for the error message when the type is unrecognised.
pub fn map_type(ty: &Type, field_name: &str) -> syn::Result<&'static str> {
    match ty {
        // Handle Type::Path: u8, u32, u64, Vec<u8>
        Type::Path(type_path) => {
            // Require no leading `::` and exactly one segment for primitives,
            // or the last segment being `Vec` for Vec<u8>.
            let segments = &type_path.path.segments;

            // Check for Vec<u8>
            if let Some(last) = segments.last() {
                if last.ident == "Vec" {
                    if let PathArguments::AngleBracketed(ref args) = last.arguments {
                        if args.args.len() == 1 {
                            if let Some(GenericArgument::Type(Type::Path(inner))) =
                                args.args.first()
                            {
                                if inner.path.is_ident("u8") {
                                    return Ok("bytes");
                                }
                            }
                        }
                    }
                }
            }

            // Check for single-segment primitives
            if segments.len() == 1 {
                let ident = &segments[0].ident;
                match ident.to_string().as_str() {
                    "u8" => return Ok("uint8"),
                    "u32" => return Ok("uint32"),
                    "u64" => return Ok("uint64"),
                    _ => {}
                }
            }

            Err(make_error(ty, field_name))
        }

        // Handle Type::Array: [u8; 33], [u8; 64], [u8; 65]
        Type::Array(type_array) => {
            // Check that the element type is u8
            let elem_is_u8 = match type_array.elem.as_ref() {
                Type::Path(p) => p.path.is_ident("u8"),
                _ => false,
            };

            if elem_is_u8 {
                // Extract the integer literal from the length expression
                if let Expr::Lit(ExprLit {
                    lit: Lit::Int(n), ..
                }) = &type_array.len
                {
                    match n.base10_parse::<usize>() {
                        Ok(33) => return Ok("secp256k1_pubkey"),
                        Ok(64) => return Ok("schnorr_sig"),
                        Ok(65) => return Ok("secp256k1_sig"),
                        _ => {}
                    }
                }
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
            if let Some(last) = segments.last() {
                if last.ident == "Vec" {
                    if let PathArguments::AngleBracketed(ref args) = last.arguments {
                        if args.args.len() == 1 {
                            if let Some(GenericArgument::Type(Type::Path(inner))) =
                                args.args.first()
                            {
                                if inner.path.is_ident("u8") {
                                    return Some(WireKind::VarBytes);
                                }
                            }
                        }
                    }
                }
            }

            // Scalar primitives
            if segments.len() == 1 {
                match segments[0].ident.to_string().as_str() {
                    "u8"  => return Some(WireKind::FixedScalar { size: 1 }),
                    "u32" => return Some(WireKind::FixedScalar { size: 4 }),
                    "u64" => return Some(WireKind::FixedScalar { size: 8 }),
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

            if elem_is_u8 {
                if let Expr::Lit(ExprLit { lit: Lit::Int(n), .. }) = &type_array.len {
                    if let Ok(size) = n.base10_parse::<usize>() {
                        return Some(WireKind::FixedArray { size });
                    }
                }
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
         supported types are: u8, u32, u64, [u8; 33], [u8; 64], [u8; 65], Vec<u8>"
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

    // Happy-path tests

    #[test]
    fn test_u8_maps_to_uint8() {
        let ty = parse("u8");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "uint8");
    }

    #[test]
    fn test_u32_maps_to_uint32() {
        let ty = parse("u32");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "uint32");
    }

    #[test]
    fn test_u64_maps_to_uint64() {
        let ty = parse("u64");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "uint64");
    }

    #[test]
    fn test_array_65_maps_to_secp256k1_sig() {
        let ty = parse("[u8; 65]");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "secp256k1_sig");
    }

    #[test]
    fn test_array_33_maps_to_secp256k1_pubkey() {
        let ty = parse("[u8; 33]");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "secp256k1_pubkey");
    }

    #[test]
    fn test_array_64_maps_to_schnorr_sig() {
        let ty = parse("[u8; 64]");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "schnorr_sig");
    }

    #[test]
    fn test_vec_u8_maps_to_bytes() {
        let ty = parse("Vec<u8>");
        assert_eq!(map_type(&ty, "my_field").unwrap(), "bytes");
    }

    // Error-path test

    #[test]
    fn test_string_produces_error() {
        let ty = parse("String");
        let err = map_type(&ty, "my_field").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unrecognised type"),
            "expected 'unrecognised type' in: {msg}"
        );
        assert!(
            msg.contains("String"),
            "expected type name 'String' in: {msg}"
        );
        assert!(
            msg.contains("my_field"),
            "expected field name 'my_field' in: {msg}"
        );
    }
}
