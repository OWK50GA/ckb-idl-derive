# ckb-idl-derive

A Rust procedural macro crate that lets CKB lock-script authors declare their witness layout directly in Rust and automatically produce a machine-readable IDL artifact at compile time.

## What it does

Add `#[derive(CkbWitness)]` to a named-field struct. At compile time the macro:

1. Reads each field's Rust type and maps it to a blessed IDL type string.
2. Collects any `#[witness(...)]` field attributes for `required` and `description` metadata.
3. Writes a `idl.json` file to `$OUT_DIR` describing the witness field array.
4. Emits a `const _CKB_WITNESS_IDL_PATH: &str` pointing to that file so downstream tooling can locate it.

No runtime code is generated beyond the path constant.

## Usage

```rust
use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
struct MyWitness {
    sig: [u8; 65],

    #[witness(required = false, description = "sender public key")]
    pubkey: [u8; 33],

    #[witness(required = false)]
    extra: Vec<u8>,
}
```

This produces `$OUT_DIR/idl.json`:

```json
{
  "witness": [
    { "name": "sig",    "type": "secp256k1_sig",    "required": true },
    { "name": "pubkey", "type": "secp256k1_pubkey",  "required": false, "description": "sender public key" },
    { "name": "extra",  "type": "bytes",             "required": false }
  ]
}
```

## Supported types

| Rust type   | IDL type string    |
|-------------|--------------------|
| `u8`        | `uint8`            |
| `u32`       | `uint32`           |
| `u64`       | `uint64`           |
| `[u8; 65]`  | `secp256k1_sig`    |
| `[u8; 33]`  | `secp256k1_pubkey` |
| `[u8; 64]`  | `schnorr_sig`      |
| `Vec<u8>`   | `bytes`            |

Using a type not in this list is a compile-time error.

## Field attributes

`#[witness(...)]` accepts the following keys on individual fields:

| Key           | Values              | Default | Description                              |
|---------------|---------------------|---------|------------------------------------------|
| `required`    | `true` / `false`    | `true`  | Whether the field must be present        |
| `description` | string literal      | —       | Human-readable description; omitted if absent |

Unrecognised keys are a compile-time error.

## Constraints

- Only named-field structs are supported. Enums, unions, tuple structs, and unit structs are rejected at compile time.
- The macro must be invoked in a Cargo build context where `OUT_DIR` is set.

## IDL output format

The generated `idl.json` is a single JSON object:

```json
{
  "witness": [ /* one object per field, in declaration order */ ]
}
```

Each field object always contains `"name"`, `"type"`, and `"required"`. The `"description"` key is present only when a description was provided — it is never `null`.

## Known limitations (v1)

- `Vec<u8>` maps to `"bytes"` with no length-framing hint (length-prefixed, fixed-tail, remainder-of-witness). A future `layout` attribute is the planned extensibility point.
- Doc comments (`///`) are not extracted as descriptions. Use `#[witness(description = "...")]` explicitly.
- Molecule-encoded witness types are not yet supported.

## Adding to your project

```toml
[dependencies]
ckb-idl-derive = { path = "../ckb-idl-derive" }   # or version once published
```

The crate is `proc-macro = true` and has no runtime surface area.
