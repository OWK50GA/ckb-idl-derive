use syn::{Data, DataEnum, DeriveInput, Fields, FieldsNamed};

pub fn check_named_struct(input: &DeriveInput) -> syn::Result<&FieldsNamed> {
    match &input.data {
        Data::Enum(_) | Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "CkbWitness can only be derived for structs",
        )),
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => Ok(f),
            _ => Err(syn::Error::new_spanned(
                input,
                "CkbWitness requires a struct with named fields",
            )),
        },
    }
}

pub fn check_enum_single_field_variants(input: &DeriveInput) -> syn::Result<&DataEnum> {
    let data_enum = match &input.data {
        Data::Enum(e) => e,
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "CkbWitnessUnion can only be derived for enums",
            ));
        }
    };

    if data_enum.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            input,
            "CkbWitnessUnion requires at least one variant",
        ));
    }

    for variant in &data_enum.variants {
        match &variant.fields {
            Fields::Unnamed(f) if f.unnamed.len() == 1 => {}
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!(
                        "variant `{}` must have exactly one unnamed field; \
                        CkbWitnessUnion does not support multi-field tuple variants",
                        variant.ident,
                    ),
                ));
            }
            Fields::Named(_) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!(
                        "variant `{}` has named fields; \
                         CkbWitnessUnion only supports single-field tuple variants",
                        variant.ident
                    ),
                ));
            }
            Fields::Unit => {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!(
                        "variant `{}` is a unit variant; \
                         CkbWitnessUnion requires each variant to carry exactly one field",
                        variant.ident
                    ),
                ));
            }
        }
    }

    Ok(data_enum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::DeriveInput;

    fn parse(input: &str) -> DeriveInput {
        syn::parse_str(input).expect("failed to parse test input")
    }

    fn unwrap_err_msg(result: syn::Result<&FieldsNamed>) -> String {
        match result {
            Ok(_) => panic!("expected Err but got Ok"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn enum_input_returns_error() {
        let input = parse("enum Foo { A, B }");
        let result = check_named_struct(&input);
        assert_eq!(
            unwrap_err_msg(result),
            "CkbWitness can only be derived for structs"
        );
    }

    #[test]
    fn union_input_returns_error() {
        let input = parse("union Foo { a: u8, b: u32 }");
        let result = check_named_struct(&input);
        assert_eq!(
            unwrap_err_msg(result),
            "CkbWitness can only be derived for structs"
        );
    }

    #[test]
    fn tuple_struct_returns_error() {
        let input = parse("struct Foo(u8, u32);");
        let result = check_named_struct(&input);
        assert_eq!(
            unwrap_err_msg(result),
            "CkbWitness requires a struct with named fields"
        );
    }

    #[test]
    fn unit_struct_returns_error() {
        let input = parse("struct Foo;");
        let result = check_named_struct(&input);
        assert_eq!(
            unwrap_err_msg(result),
            "CkbWitness requires a struct with named fields"
        );
    }

    #[test]
    fn named_field_struct_returns_ok() {
        let input = parse("struct Foo { a: u8, b: u32 }");
        let result = check_named_struct(&input);
        assert!(result.is_ok());
        let fields = result.unwrap();
        assert_eq!(fields.named.len(), 2);
    }
}
