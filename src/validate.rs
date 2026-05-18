use syn::{Data, DeriveInput, Fields, FieldsNamed};

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
