use std::path::PathBuf;

/// Write `json` to `$CARGO_MANIFEST_DIR/idl.json` and return the absolute path.
///
/// `CARGO_MANIFEST_DIR` is set by Cargo for every crate during proc-macro
/// expansion — no `build.rs` is required in the consumer crate.
///
/// The file lands next to the crate's `Cargo.toml`, which is predictable and
/// stable across rebuilds (unlike the hashed `OUT_DIR` path).
///
/// Errors:
/// - `CARGO_MANIFEST_DIR` not set → `"CARGO_MANIFEST_DIR environment variable is not set"`
/// - write failure               → `"failed to write IDL file to \`<path>\`: <io::Error>"`
pub fn write_idl(json: &str) -> syn::Result<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "CARGO_MANIFEST_DIR environment variable is not set",
        )
    })?;

    let path = PathBuf::from(manifest_dir).join("idl.json");

    std::fs::write(&path, json.as_bytes()).map_err(|e| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to write IDL file to `{}`: {e}", path.display()),
        )
    })?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_write_returns_path() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        unsafe { std::env::set_var("CARGO_MANIFEST_DIR", dir.path().to_str().unwrap()) };

        let json = r#"{"witness":[]}"#;
        let result = write_idl(json);

        unsafe { std::env::remove_var("CARGO_MANIFEST_DIR") };

        let path = result.expect("write_idl should succeed");
        assert_eq!(path.file_name().unwrap(), "idl.json");
        assert_eq!(path.parent().unwrap(), dir.path());

        let content = std::fs::read_to_string(&path).expect("should be able to read written file");
        assert_eq!(content, json);
    }

    #[test]
    fn missing_manifest_dir_returns_error() {
        unsafe { std::env::remove_var("CARGO_MANIFEST_DIR") };

        let err = write_idl("{}").unwrap_err();
        assert_eq!(
            err.to_string(),
            "CARGO_MANIFEST_DIR environment variable is not set"
        );
    }
}
