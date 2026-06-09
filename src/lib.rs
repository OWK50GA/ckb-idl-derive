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

            Ok(FieldMeta {
                name: field_name,
                idl_type,
                required: attrs.required,
                description: attrs.description,
                wire_kind,
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

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

/// Public proc-macro entry point.
#[proc_macro_derive(CkbWitness, attributes(witness))]
pub fn ckb_witness(input: TokenStream) -> TokenStream {
    let input = TokenStream2::from(input);
    match impl_ckb_witness(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Helper: run `impl_ckb_witness` with a real temp OUT_DIR.
    fn run_with_tempdir(input: TokenStream2) -> syn::Result<TokenStream2> {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        // SAFETY: tests in this module run single-threaded (no parallel test
        // threads share OUT_DIR at the same time for this crate's test binary).
        unsafe { std::env::set_var("OUT_DIR", dir.path().to_str().unwrap()) };
        let result = impl_ckb_witness(input);
        unsafe { std::env::remove_var("OUT_DIR") };
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
