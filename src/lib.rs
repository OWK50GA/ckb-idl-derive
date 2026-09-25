use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;

mod attr;
mod codegen;
mod io;
mod registry;
mod validate;

// Re-export FieldMeta so tests in this file can use it.
use codegen::FieldMeta;

/// Parse fields into the single representation used by both top-level and
/// inner witnesses. Keeping this here prevents the two derives from accepting
/// materially different layouts.
fn collect_field_metas(fields_named: &syn::FieldsNamed) -> syn::Result<Vec<FieldMeta>> {
    let metas = fields_named
        .named
        .iter()
        .map(|field| {
            let field_ident = field.ident.as_ref().expect("named field has no ident");
            let field_name = attr::idl_identifier(field_ident)?;
            let attrs = attr::parse_field_attrs(field)?;
            let idl_type = registry::map_type(&field.ty, &field_name)?;
            let wire_kind = registry::map_wire_kind(&field.ty)
                .expect("map_wire_kind must succeed for any type accepted by map_type");

            let wire_kind = if attrs.is_union {
                match wire_kind {
                    registry::WireKind::Struct(path) => registry::WireKind::Union(path),
                    _ => {
                        return Err(syn::Error::new_spanned(
                            &field.ty,
                            format!(
                                "field `{field_name}` is marked `#[witness(union)]` but its type \
                                 is not a named path type; only named enum types are supported"
                            ),
                        ));
                    }
                }
            } else {
                wire_kind
            };

            if attrs.is_union && attrs.type_override.is_some() {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "field `{field_name}` cannot combine `#[witness(union)]` with a semantic type override"
                    ),
                ));
            }
            if let Some(label) = &attrs.type_override {
                attr::validate_semantic_type(label, &idl_type, field_ident.span())?;
            }

            let is_optional_type = matches!(&wire_kind, registry::WireKind::Optional(_));
            if is_optional_type && attrs.required {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "field `{field_name}` is `Option<T>` but `required = true`; \
                         either add `#[witness(required = false)]` or use a non-optional type"
                    ),
                ));
            }
            if !is_optional_type && !attrs.required {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "field `{field_name}` is marked `required = false` but its type is not \
                         `Option<T>`; wrap the type in `Option<...>` or remove `required = false`"
                    ),
                ));
            }
            if matches!(&wire_kind, registry::WireKind::Optional(inner) if matches!(inner.as_ref(), registry::WireKind::Struct(_))) {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "field `{field_name}` is `Option<NamedStruct>` which is not supported; \
                         optional nested structs have no safe wire boundary under the \
                         trailing-exhaustion encoding — use `Option<Vec<u8>>` and decode manually"
                    ),
                ));
            }
            if matches!(&wire_kind, registry::WireKind::Optional(inner) if matches!(inner.as_ref(), registry::WireKind::VecOf(_))) {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "field `{field_name}` is `Option<Vec<T>>` where T is not a u8, which is not \
                         supported; use a required Vec<T> or `Option<Vec<u8>>` instead"
                    ),
                ));
            }

            Ok(FieldMeta {
                ident: field_ident.clone(),
                name: field_name,
                required: attrs.required,
                description: attrs.description,
                type_override: attrs.type_override,
                wire_kind,
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let mut seen_optional = false;
    let mut seen_nested: Option<&str> = None;
    for (field, meta) in fields_named.named.iter().zip(&metas) {
        let is_optional = matches!(&meta.wire_kind, registry::WireKind::Optional(_));
        let is_nested = matches!(
            &meta.wire_kind,
            registry::WireKind::Struct(_) | registry::WireKind::Union(_)
        );

        if let Some(nested_name) = seen_nested {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!(
                    "field `{}` appears after nested struct field `{nested_name}`; \
                     a nested struct field must be the last field in the struct \
                     because its optional trailing fields use buffer exhaustion",
                    meta.name
                ),
            ));
        }
        if is_optional {
            seen_optional = true;
        } else if seen_optional {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!(
                    "required field `{}` appears after an optional field; \
                     all `Option<T>` fields must come last in the struct",
                    meta.name
                ),
            ));
        }
        if is_nested && seen_optional {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!(
                    "nested struct field `{}` appears after an optional field; \
                     nested struct fields must come before all `Option<T>` fields",
                    meta.name
                ),
            ));
        }
        if is_nested {
            seen_nested = Some(&meta.name);
        }
    }

    Ok(metas)
}

/// Internal implementation — takes a `proc_macro2::TokenStream` so it can be
/// called from unit/property tests without going through the proc-macro host.
fn impl_ckb_witness(input: TokenStream2) -> syn::Result<TokenStream2> {
    let ast = syn::parse2::<DeriveInput>(input)?;

    // 1. Validate: must be a named-field struct.
    let fields_named = validate::check_named_struct(&ast)?;

    // 2. Parse and validate the shared field layout.
    let metas = collect_field_metas(fields_named)?;

    let impl_ts = codegen::emit_impl(&ast.ident, &metas);
    let recursive = metas.iter().any(|meta| {
        matches!(
            meta.wire_kind,
            registry::WireKind::Struct(_) | registry::WireKind::Union(_)
        )
    });
    if recursive {
        // The proc macro cannot inspect separately declared nested types.
        // `ckb-idl-export` is the authoritative artifact producer here.
        io::remove_stale_idl()?;
        return Ok(impl_ts);
    }

    let idl = codegen::build_idl(&metas);
    let json = serde_json_canonicalizer::to_string(&idl)
        .expect("canonical JSON serialisation is infallible for this value");
    let path = io::write_idl(&json)?;
    let mut out = codegen::emit_const(&path);
    out.extend(impl_ts);
    Ok(out)
}

fn impl_ckb_inner_witness(input: TokenStream2) -> syn::Result<TokenStream2> {
    let ast = syn::parse2::<DeriveInput>(input)?;

    // Same validation as CkbWitness i.e. must be a named-field struct
    let fields_named = validate::check_named_struct(&ast)?;

    let metas = collect_field_metas(fields_named)?;

    // No idl.json written, instead offloaded to CkbWitness
    // Also no from_witness_args emitted, that is not our concern
    // Emit only the WitnessFields trait impl.
    Ok(codegen::emit_inner_impl(&ast.ident, &metas))
}

fn impl_ckb_witness_union(input: TokenStream2) -> syn::Result<TokenStream2> {
    let ast = syn::parse2::<DeriveInput>(input)?;

    // Validate: must be an enum with single-field tuple variants.
    let data_enum = validate::check_enum_single_field_variants(&ast)?;

    let tags = data_enum
        .variants
        .iter()
        .map(attr::parse_union_variant_tag)
        .collect::<syn::Result<Vec<_>>>()?;

    for variant in &data_enum.variants {
        attr::idl_identifier(&variant.ident)?;
    }

    for (index, tag) in tags.iter().enumerate() {
        if tags[..index].contains(tag) {
            let variant = &data_enum.variants[index];
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("union tag `{tag}` is already used by an earlier variant"),
            ));
        }
    }

    // Emit the WitnessUnion trait impl.
    Ok(codegen::emit_union_impl(
        &ast.ident,
        &data_enum.variants,
        &tags,
    ))
}

/// Public proc-macro entry point.
#[proc_macro_derive(CkbWitness, attributes(witness))]
pub fn ckb_witness(input: TokenStream) -> TokenStream {
    let input = TokenStream2::from(input);
    match impl_ckb_witness(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[proc_macro_derive(CkbInnerWitness, attributes(witness))]
pub fn ckb_inner_witness(input: TokenStream) -> TokenStream {
    let input = TokenStream2::from(input);
    match impl_ckb_inner_witness(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[proc_macro_derive(CkbWitnessUnion, attributes(witness))]
pub fn ckb_witness_union(input: TokenStream) -> TokenStream {
    let input = TokenStream2::from(input);
    match impl_ckb_witness_union(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Helper: run `impl_ckb_witness` with a real temp CARGO_MANIFEST_DIR.
    fn run_with_tempdir(input: TokenStream2) -> syn::Result<TokenStream2> {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        // SAFETY: tests in this module run single-threaded (no parallel test
        // threads share CARGO_MANIFEST_DIR at the same time for this crate's test binary).
        unsafe { std::env::set_var("CARGO_MANIFEST_DIR", dir.path().to_str().unwrap()) };
        let result = impl_ckb_witness(input);
        unsafe { std::env::remove_var("CARGO_MANIFEST_DIR") };
        // Keep `dir` alive until after the call so the path is valid.
        drop(dir);
        result
    }

    // ── Task 8.2 — Property 4: Generated const presence ─────────────────────

    proptest! {
        #[test]
        fn prop4_generated_const_presence(
            struct_name in "[A-Z][a-zA-Z0-9]{1,15}",
            field_names in proptest::collection::vec("[a-z][a-z0-9_]{0,10}", 1..=8),
        ) {
            // Build a valid named-field struct TokenStream with u8 fields.
            // Skip any generated name that happens to be a Rust keyword.
            let rust_keywords = [
                "as", "break", "const", "continue", "crate", "else", "enum", "extern",
                "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
                "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super",
                "trait", "true", "type", "unsafe", "use", "where", "while",
                // reserved / future keywords
                "abstract", "become", "box", "do", "final", "macro", "override", "priv",
                "try", "typeof", "unsized", "virtual", "yield",
            ];

            let valid_names: Vec<&str> = field_names
                .iter()
                .map(|s| s.as_str())
                .filter(|s| !rust_keywords.contains(s))
                .collect();

            // If all generated names were keywords, skip this case.
            prop_assume!(!valid_names.is_empty());

            let fields_ts: TokenStream2 = valid_names
                .iter()
                .map(|n| {
                    let ident = syn::parse_str::<syn::Ident>(n).unwrap();
                    quote::quote! { #ident: u8, }
                })
                .collect();

            let struct_ident = syn::parse_str::<syn::Ident>(&struct_name).unwrap();
            let input: TokenStream2 = quote::quote! {
                struct #struct_ident { #fields_ts }
            };
            let ts = run_with_tempdir(input).expect("impl_ckb_witness should succeed");
            let ts_str = ts.to_string();

            prop_assert!(
                ts_str.contains("_CKB_WITNESS_IDL_PATH"),
                "output does not contain _CKB_WITNESS_IDL_PATH: {ts_str}"
            );
            prop_assert!(
                ts_str.contains("& str") || ts_str.contains("&str"),
                "output does not contain &str: {ts_str}"
            );
        }
    }

    #[test]
    fn duplicate_union_tags_are_rejected() {
        let input: TokenStream2 = quote::quote! {
            enum Authorization {
                #[witness(tag = 7)]
                First(FirstPayload),
                #[witness(tag = 7)]
                Second(SecondPayload),
            }
        };
        let err = impl_ckb_witness_union(input).unwrap_err();
        assert!(err.to_string().contains("union tag `7` is already used"));
    }
}
