use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Ident, Lit, LitBool, LitStr, Token,
};

/// Parsed field-level `#[witness(...)]` attributes.
#[derive(Debug)]
pub struct FieldAttrs {
    /// Whether the field is required in the witness. Defaults to `true`.
    pub required: bool,
    /// Optional human-readable description.
    pub description: Option<String>,
}

/// A single `key = value` pair inside `#[witness(...)]`.
struct KeyValue {
    key: Ident,
    value: Lit,
}

impl Parse for KeyValue {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let value: Lit = input.parse()?;
        Ok(KeyValue { key, value })
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

    for attr in &field.attrs {
        if !attr.path().is_ident("witness") {
            continue;
        }

        let args: WitnessArgs = attr.parse_args_with(WitnessArgs::parse)?;

        for kv in args.0 {
            let key_str = kv.key.to_string();
            match key_str.as_str() {
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
                _ => {
                    return Err(syn::Error::new_spanned(
                        &kv.key,
                        format!(
                            "unrecognised witness attribute key `{key_str}`; \
                             supported keys are: required, description"
                        ),
                    ));
                }
            }
        }
    }

    Ok(FieldAttrs {
        required,
        description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{parse_quote, ItemStruct};

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
        let f = first_field(s);
        let attrs = parse_field_attrs(&f).unwrap();
        assert!(attrs.required);
        assert!(attrs.description.is_none());
    }

    #[test]
    fn required_false() {
        let s: ItemStruct = parse_quote! { struct S { #[witness(required = false)] x: u8 } };
        let f = first_field(s);
        let attrs = parse_field_attrs(&f).unwrap();
        assert!(!attrs.required);
        assert!(attrs.description.is_none());
    }

    #[test]
    fn no_attribute_defaults_required_true() {
        let s: ItemStruct = parse_quote! { struct S { x: u8 } };
        let f = first_field(s);
        let attrs = parse_field_attrs(&f).unwrap();
        assert!(attrs.required);
        assert!(attrs.description.is_none());
    }

    #[test]
    fn description_only() {
        let s: ItemStruct =
            parse_quote! { struct S { #[witness(description = "some text")] x: u8 } };
        let f = first_field(s);
        let attrs = parse_field_attrs(&f).unwrap();
        assert!(attrs.required); // default
        assert_eq!(attrs.description.as_deref(), Some("some text"));
    }

    #[test]
    fn combined_required_false_and_description() {
        let s: ItemStruct =
            parse_quote! { struct S { #[witness(required = false, description = "hello")] x: u8 } };
        let f = first_field(s);
        let attrs = parse_field_attrs(&f).unwrap();
        assert!(!attrs.required);
        assert_eq!(attrs.description.as_deref(), Some("hello"));
    }

    #[test]
    fn unrecognised_key_returns_error() {
        let s: ItemStruct = parse_quote! { struct S { #[witness(foo = "bar")] x: u8 } };
        let f = first_field(s);
        let err = parse_field_attrs(&f).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unrecognised witness attribute key `foo`"),
            "unexpected error message: {msg}"
        );
        assert!(
            msg.contains("supported keys are: required, description"),
            "unexpected error message: {msg}"
        );
    }
}
