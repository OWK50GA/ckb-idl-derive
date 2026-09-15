use syn::{
    Ident, Lit, LitBool, LitStr, Token,
    ext::IdentExt,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// Parsed field-level `#[witness(...)]` attributes.
#[derive(Debug)]
pub struct FieldAttrs {
    /// Whether the field is required in the witness. Defaults to `true`.
    pub required: bool,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Optional semantic type label override.
    ///
    /// When present, this string is used as the IDL `"type"` value instead of
    /// the structural default derived from the Rust type (e.g. `"bytes_fixed_65"`).
    /// The macro does not validate the string — it is the author's statement of
    /// semantic intent (e.g. `"secp256k1_sig"`, `"blake2b_hash"`, `"my_proof"`).
    pub type_override: Option<String>,
}

/// A single `key = value` pair inside `#[witness(...)]`.
/// The key is stored as a plain `String` because `type` is a Rust keyword and
/// `Ident::parse` (the default) rejects reserved words. `Ident::parse_any`
/// accepts keywords, giving us the identifier text without a compile error.
struct KeyValue {
    key: String,
    key_span: proc_macro2::Span,
    value: Lit,
}

impl Parse for KeyValue {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // parse_any accepts reserved keywords like `type` as identifiers.
        let key_ident = Ident::parse_any(input)?;
        let key_span = key_ident.span();
        let key = key_ident.to_string();
        let _eq: Token![=] = input.parse()?;
        let value: Lit = input.parse()?;
        Ok(KeyValue {
            key,
            key_span,
            value,
        })
    }
}

/// A comma-separated list of `key = value` pairs.
struct WitnessArgs(Punctuated<KeyValue, Token![,]>);

impl Parse for WitnessArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(WitnessArgs(
            Punctuated::<KeyValue, Token![,]>::parse_terminated(input)?,
        ))
    }
}

/// Parse all `#[witness(...)]` attributes on a field and merge them into
/// a single `FieldAttrs`. Multiple attributes are allowed; last-write-wins
/// for duplicate keys.
pub fn parse_field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut required = true;
    let mut description: Option<String> = None;
    let mut type_override: Option<String> = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("witness") {
            continue;
        }

        let args: WitnessArgs = attr.parse_args_with(WitnessArgs::parse)?;

        for kv in args.0 {
            let key_str = kv.key.as_str();
            match key_str {
                "required" => {
                    if let Lit::Bool(LitBool { value, .. }) = kv.value {
                        required = value;
                    } else {
                        return Err(syn::Error::new_spanned(
                            &kv.value,
                            "expected a boolean literal for `required`",
                        ));
                    }
                }
                "description" => {
                    if let Lit::Str(LitStr { .. }) = &kv.value {
                        if let Lit::Str(s) = kv.value {
                            description = Some(s.value());
                        }
                    } else {
                        return Err(syn::Error::new_spanned(
                            &kv.value,
                            "expected a string literal for `description`",
                        ));
                    }
                }
                "type" => {
                    if let Lit::Str(s) = kv.value {
                        let label = s.value();
                        if label.is_empty() {
                            return Err(syn::Error::new_spanned(
                                &s,
                                "`type` override must not be an empty string",
                            ));
                        }
                        type_override = Some(label);
                    } else {
                        return Err(syn::Error::new_spanned(
                            &kv.value,
                            "expected a string literal for `type`",
                        ));
                    }
                }
                _ => {
                    return Err(syn::Error::new(
                        kv.key_span,
                        format!(
                            "unrecognised witness attribute key `{key_str}`; \
                             supported keys are: required, description, type"
                        ),
                    ));
                }
            }
        }
    }

    Ok(FieldAttrs {
        required,
        description,
        type_override,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{ItemStruct, parse_quote};

    /// Extract the first field from a struct definition.
    fn first_field(s: ItemStruct) -> syn::Field {
        match s.fields {
            syn::Fields::Named(f) => f.named.into_iter().next().unwrap(),
            _ => panic!("expected named fields"),
        }
    }

    #[test]
    fn required_true() {
        let s: ItemStruct = parse_quote! { struct S { #[witness(required = true)] x: u8 } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(attrs.required);
        assert!(attrs.description.is_none());
        assert!(attrs.type_override.is_none());
    }

    #[test]
    fn required_false() {
        let s: ItemStruct = parse_quote! { struct S { #[witness(required = false)] x: u8 } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(!attrs.required);
        assert!(attrs.description.is_none());
        assert!(attrs.type_override.is_none());
    }

    #[test]
    fn no_attribute_defaults_required_true() {
        let s: ItemStruct = parse_quote! { struct S { x: u8 } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(attrs.required);
        assert!(attrs.description.is_none());
        assert!(attrs.type_override.is_none());
    }

    #[test]
    fn description_only() {
        let s: ItemStruct =
            parse_quote! { struct S { #[witness(description = "some text")] x: u8 } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(attrs.required); // default
        assert_eq!(attrs.description.as_deref(), Some("some text"));
        assert!(attrs.type_override.is_none());
    }

    #[test]
    fn combined_required_false_and_description() {
        let s: ItemStruct =
            parse_quote! { struct S { #[witness(required = false, description = "hello")] x: u8 } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(!attrs.required);
        assert_eq!(attrs.description.as_deref(), Some("hello"));
        assert!(attrs.type_override.is_none());
    }

    #[test]
    fn type_override_only() {
        let s: ItemStruct =
            parse_quote! { struct S { #[witness(type = "secp256k1_sig")] sig: [u8; 65] } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(attrs.required); // default
        assert!(attrs.description.is_none());
        assert_eq!(attrs.type_override.as_deref(), Some("secp256k1_sig"));
    }

    #[test]
    fn type_override_combined_with_description() {
        let s: ItemStruct = parse_quote! {
            struct S {
                #[witness(type = "schnorr_sig", description = "64-byte Schnorr signature")]
                sig: [u8; 64]
            }
        };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(attrs.required);
        assert_eq!(
            attrs.description.as_deref(),
            Some("64-byte Schnorr signature")
        );
        assert_eq!(attrs.type_override.as_deref(), Some("schnorr_sig"));
    }

    #[test]
    fn type_override_arbitrary_label() {
        // The macro does not validate the label — any non-empty string is accepted.
        let s: ItemStruct =
            parse_quote! { struct S { #[witness(type = "my_custom_proof")] proof: [u8; 32] } };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert_eq!(attrs.type_override.as_deref(), Some("my_custom_proof"));
    }

    #[test]
    fn type_override_all_three_keys() {
        let s: ItemStruct = parse_quote! {
            struct S {
                #[witness(required = false, type = "blake2b_hash", description = "optional hash")]
                h: [u8; 32]
            }
        };
        let attrs = parse_field_attrs(&first_field(s)).unwrap();
        assert!(!attrs.required);
        assert_eq!(attrs.type_override.as_deref(), Some("blake2b_hash"));
        assert_eq!(attrs.description.as_deref(), Some("optional hash"));
    }

    #[test]
    fn type_override_empty_string_is_error() {
        let s: ItemStruct = parse_quote! { struct S { #[witness(type = "")] x: [u8; 32] } };
        let err = parse_field_attrs(&first_field(s)).unwrap_err();
        assert!(
            err.to_string().contains("must not be an empty string"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unrecognised_key_returns_error() {
        let s: ItemStruct = parse_quote! { struct S { #[witness(foo = "bar")] x: u8 } };
        let err = parse_field_attrs(&first_field(s)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unrecognised witness attribute key `foo`"),
            "unexpected error message: {msg}"
        );
        assert!(
            msg.contains("supported keys are: required, description, type"),
            "unexpected error message: {msg}"
        );
    }
}
