use std::path::PathBuf;

/// Write `json` to `$OUT_DIR/idl.json` and return the absolute path.
///
/// Errors:
/// - `OUT_DIR` not set → `"OUT_DIR environment variable is not set"`
/// - write failure    → `"failed to write IDL file to \`<path>\`: <io::Error>"`
pub fn write_idl(json: &str) -> syn::Result<PathBuf> {
    let out_dir = std::env::var("OUT_DIR").map_err(|_| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "OUT_DIR environment variable is not set",
        )
    })?;

    let path = PathBuf::from(out_dir).join("idl.json");

    std::fs::write(&path, json.as_bytes()).map_err(|e| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "failed to write IDL file to `{}`: {e}",
                path.display()
            ),
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
        // Set OUT_DIR to the temp directory for this test
        // SAFETY: single-threaded test; no other threads reading OUT_DIR concurrently.
        unsafe { std::env::set_var("OUT_DIR", dir.path().to_str().unwrap()) };

        let json = r#"{"witness":[]}"#;
        let result = write_idl(json);

        // Restore: remove OUT_DIR so other tests aren't affected
        unsafe { std::env::remove_var("OUT_DIR") };

        let path = result.expect("write_idl should succeed");
        assert_eq!(path.file_name().unwrap(), "idl.json");
        assert_eq!(path.parent().unwrap(), dir.path());

        // Verify the file content
        let content = std::fs::read_to_string(&path).expect("should be able to read written file");
        assert_eq!(content, json);
    }

    #[test]
    fn missing_out_dir_returns_error() {
        // Ensure OUT_DIR is not set
        // SAFETY: single-threaded test; no other threads reading OUT_DIR concurrently.
        unsafe { std::env::remove_var("OUT_DIR") };

        let err = write_idl("{}").unwrap_err();
        assert_eq!(err.to_string(), "OUT_DIR environment variable is not set");
    }
}
