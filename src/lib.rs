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

/// Internal implementation — takes a `proc_macro2::TokenStream` so it can be
/// called from unit/property tests without going through the proc-macro host.
fn impl_ckb_witness(input: TokenStream2) -> syn::Result<TokenStream2> {
    let ast = syn::parse2::<DeriveInput>(input)?;

    // 1. Validate: must be a named-field struct.
    let fields_named = validate::check_named_struct(&ast)?;

    // 2. For each field: parse attributes + map type → FieldMeta.
    let metas = fields_named
        .named
        .iter()
        .map(|f| {
            let field_name = f
                .ident
                .as_ref()
                .expect("named field has no ident")
                .to_string();

            let attrs = attr::parse_field_attrs(f)?;
            let idl_type = registry::map_type(&f.ty, &field_name)?;
            let wire_kind = registry::map_wire_kind(&f.ty)
                .expect("map_wire_kind must succeed for any type accepted by map_type");

            // 2a. Consistency check: required ↔ Option<T>
            let is_optional_type = matches!(wire_kind, registry::WireKind::Optional(_));
            if is_optional_type && attrs.required {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    format!(
                        "field `{field_name}` is `Option<T>` but `required = true`; \
                         either add `#[witness(required = false)]` or use a non-optional type"
                    ),
                ));
            }
            if !is_optional_type && !attrs.required {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    format!(
                        "field `{field_name}` is marked `required = false` but its type is not \
                         `Option<T>`; wrap the type in `Option<...>` or remove `required = false`"
                    ),
                ));
            }
            // Option<NamedStruct> is not supported: the Optional decoder has
            // no WireKind::Struct arm and there is no safe framing convention
            // for an optional nested struct under the trailing-exhaustion model.
            if matches!(&wire_kind, registry::WireKind::Optional(inner) if matches!(inner.as_ref(), registry::WireKind::Struct(_))) {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    format!(
                        "field `{field_name}` is `Option<NamedStruct>` which is not supported; \
                         optional nested structs have no safe wire boundary under the \
                         trailing-exhaustion encoding — use `Option<Vec<u8>>` and decode manually"
                    ),
                ));
            }

            // Option<Vec<T>> where T is not u8 is also not supported - the element
            // count prefix inside the optional makes the trailing-exhaustion boundary 
            // unreliable when followed by other fields.
            if matches!(&wire_kind, registry::WireKind::Optional(inner) if matches!(inner.as_ref(), registry::WireKind::VecOf(_))) {
                return Err(syn::Error::new_spanned(
                    &f.ty, 
                    format!(
                        "field `{field_name}` is `Option<Vec<T>>` where T is not a u8, which is not\
                        supported; use a required Vec<T> or `Option<Vec<u8>>` instead"
                    )
                ));
            }

            Ok(FieldMeta {
                name: field_name,
                idl_type,
                required: attrs.required,
                description: attrs.description,
                type_override: attrs.type_override,
                wire_kind,
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    // 2b. Ordering check: Optional fields must come after all required fields.
    // The buffer-exhaustion convention used to decode None only works when
    // there are no required fields after the optional ones.
    // Additionally, a Struct field (CkbInnerWitness) that contains optional
    // trailing fields must itself be the last non-optional field in the parent,
    // for the same reason — we cannot distinguish its trailing exhaustion from
    // the start of the next parent field. For simplicity we reject Struct fields
    // that appear before any optional field in the parent.
    let mut seen_optional = false;
    for meta in &metas {
        let is_opt = matches!(meta.wire_kind, registry::WireKind::Optional(_));
        let is_struct = matches!(meta.wire_kind, registry::WireKind::Struct(_));
        if is_opt {
            seen_optional = true;
        } else if seen_optional {
            let offending = fields_named
                .named
                .iter()
                .find(|f| f.ident.as_ref().map(|i| i.to_string()).as_deref() == Some(&meta.name))
                .expect("field must exist");
            return Err(syn::Error::new_spanned(
                &offending.ty,
                format!(
                    "required field `{}` appears after an optional field; \
                     all `Option<T>` fields must come last in the struct",
                    meta.name
                ),
            ));
        }
        // A nested struct field must come before any optional field in the
        // parent. If it appears after an optional field the boundary is
        // ambiguous; if it appears before one the inner struct's own optional
        // trailing fields would consume parent bytes. Struct fields must come
        // before all optional fields in the parent layout.
        if is_struct && seen_optional {
            let offending = fields_named
                .named
                .iter()
                .find(|f| f.ident.as_ref().map(|i| i.to_string()).as_deref() == Some(&meta.name))
                .expect("field must exist");
            return Err(syn::Error::new_spanned(
                &offending.ty,
                format!(
                    "nested struct field `{}` appears after an optional field; \
                     nested struct fields must come before all `Option<T>` fields",
                    meta.name
                ),
            ));
        }
    }

    // 3. Build IDL JSON and serialise.
    let idl = codegen::build_idl(&metas);
    let json =
        serde_json::to_string(&idl).expect("serde_json serialisation is infallible for this value");

    // 4. Write idl.json to OUT_DIR.
    let path = io::write_idl(&json)?;

    // 5. Emit the path constant + from_witness_args impl.
    let const_ts = codegen::emit_const(&path);
    let impl_ts = codegen::emit_impl(&ast.ident, &metas);

    let mut out = const_ts;
    out.extend(impl_ts);
    Ok(out)
}

fn impl_ckb_inner_witness(input: TokenStream2) -> syn::Result<TokenStream2> {
    let ast = syn::parse2::<DeriveInput>(input)?;

    // Same validation as CkbWitness i.e. must be a named-field struct
    let fields_named = validate::check_named_struct(&ast)?;

    // Same field parsing pipeline as CkbWitness
    let metas = fields_named
        .named
        .iter()
        .map(|f| {
            let field_name = f
                .ident
                .as_ref()
                .expect("named field has no ident")
                .to_string();

            let attrs = attr::parse_field_attrs(f)?;
            let idl_type = registry::map_type(&f.ty, &field_name)?;
            let wire_kind = registry::map_wire_kind(&f.ty)
                .expect("map_wire_kind must succeed for any type accepted by map_type");

            let is_optional_type = matches!(wire_kind, registry::WireKind::Optional(_));
            if is_optional_type && attrs.required {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    format!(
                        "field `{field_name}` is `Option<T>` but `required = true`; \
                         either add `#[witness(required = false)]` or use a non-optional type"
                    ),
                ));
            }
            if !is_optional_type && !attrs.required {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    format!(
                        "field `{field_name}` is marked `required = false` but its type is not \
                         `Option<T>`; wrap the type in `Option<...>` or remove `required = false`"
                    ),
                ));
            }
            // Option<NamedStruct> is not supported: the Optional decoder has
            // no WireKind::Struct arm and there is no safe framing convention
            // for an optional nested struct under the trailing-exhaustion model.
            if matches!(&wire_kind, registry::WireKind::Optional(inner) if matches!(inner.as_ref(), registry::WireKind::Struct(_))) {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    format!(
                        "field `{field_name}` is `Option<NamedStruct>` which is not supported; \
                         optional nested structs have no safe wire boundary under the \
                         trailing-exhaustion encoding — use `Option<Vec<u8>>` and decode manually"
                    ),
                ));
            }

            Ok(FieldMeta {
                name: field_name,
                idl_type,
                required: attrs.required,
                description: attrs.description,
                type_override: attrs.type_override,
                wire_kind
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    // Same ordering check as CkbWitness
    let mut seen_optional = false;
    for meta in &metas {
        let is_opt = matches!(meta.wire_kind, registry::WireKind::Optional(_));
        if is_opt {
            seen_optional = true;
        } else if seen_optional {
            let offending = fields_named
                .named
                .iter()
                .find(|f| f.ident.as_ref().map(|i| i.to_string()).as_deref() == Some(&meta.name))
                .expect("field must exist");
            return Err(syn::Error::new_spanned(
                &offending.ty,
                format!(
                    "required field `{}` appears after an optional field; \
                     all `Option<T>` fields must come last in the struct",
                    meta.name
                ),
            ));
        }
    }

    // No idl.json written, instead offloaded to CkbWitness
    // Also no from_witness_args emitted, that is not our concern
    // Emit only the WitnessFields trait impl.
    Ok(codegen::emit_inner_impl(&ast.ident, &metas))
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
}
