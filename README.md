# ckb-idl-derive

A Rust procedural macro that lets CKB lock-script authors declare their witness layout directly in Rust and automatically generate a machine-readable IDL artifact at compile time.

This is the script side of the CKB IDL system. The wallet/tooling side is [`ckb-idl-client`](https://github.com/your-org/ckb-idl-client).

---

## What it does

Add `#[derive(CkbWitness)]` to a named-field struct. At compile time the macro:

1. Maps each field's Rust type to a canonical IDL type string.
2. Collects `#[witness(...)]` field attributes for `required` and `description` metadata.
3. Writes an `idl.json` file to `CARGO_MANIFEST_DIR` describing the witness field array.
4. Generates a `from_witness_args(index, source)` method that deserialises the witness from the CKB VM at runtime.

The `idl.json` is the artifact that gets committed on-chain at deployment time. The `from_witness_args` method is what the script calls inside the VM to decode its inputs.

---

## Usage

```toml
[dependencies]
ckb-idl-derive = { path = "../ckb-idl-derive" }
ckb-idl-types  = { path = "../ckb-idl-derive/ckb-idl-types" }
```

```rust
use ckb_idl_derive::CkbWitness;

#[derive(CkbWitness)]
pub struct Witness {
    /// 65-byte ECDSA signature over the transaction hash.
    #[witness(description = "secp256k1 ECDSA signature authorising the spend")]
    pub signature: [u8; 65],

    /// Earliest timestamp (ms) at which this cell may be spent.
    #[witness(description = "Unix timestamp in milliseconds; cell cannot be spent before this")]
    pub unlock_after_ms: u64,

    /// Optional auxiliary payload.
    #[witness(required = false, description = "Optional extra payload")]
    pub extra: Vec<u8>,
}
```

This produces `idl.json` next to `Cargo.toml`:

```json
{
  "witness": [
    { "name": "signature",       "type": "secp256k1_sig", "required": true,  "description": "secp256k1 ECDSA signature authorising the spend" },
    { "name": "unlock_after_ms", "type": "uint64",        "required": true,  "description": "Unix timestamp in milliseconds; cell cannot be spent before this" },
    { "name": "extra",           "type": "bytes",         "required": false, "description": "Optional extra payload" }
  ]
}
```

And it generates this method on the struct:

```rust
impl Witness {
    pub fn from_witness_args(
        index: usize,
        source: ckb_std::ckb_constants::Source,
    ) -> Result<Self, ckb_idl_types::WitnessError> { ... }
}
```

Inside the script, call it like:

```rust
let witness = Witness::from_witness_args(0, Source::GroupInput)?;
// Now use witness.signature, witness.unlock_after_ms, witness.extra
```

---

## Supported types

These are all the Rust types the macro currently accepts. Using anything else is a compile-time error.

| Rust type   | IDL type string    | Wire encoding                                 |
|-------------|--------------------|-----------------------------------------------|
| `u8`        | `uint8`            | 1 byte                                        |
| `u32`       | `uint32`           | 4 bytes, little-endian                        |
| `u64`       | `uint64`           | 8 bytes, little-endian                        |
| `[u8; 65]`  | `secp256k1_sig`    | 65 bytes, fixed                               |
| `[u8; 33]`  | `secp256k1_pubkey` | 33 bytes, fixed                               |
| `[u8; 64]`  | `schnorr_sig`      | 64 bytes, fixed                               |
| `Vec<u8>`   | `bytes`            | 4-byte LE length prefix + payload             |

The type registry is the single source of truth for both the IDL generator and the wire decoder. If you need a type that isn't here (Molecule-encoded data, fixed-length hashes, custom structs), it must be added to the registry before the macro can accept it.

---

## Field attributes

`#[witness(...)]` accepts the following keys:

| Key           | Values           | Default | Description                                         |
|---------------|------------------|---------|-----------------------------------------------------|
| `required`    | `true` / `false` | `true`  | Whether the field must be present in the witness    |
| `description` | string literal   | none    | Human-readable description; omitted if not provided |

Unrecognised keys are a compile-time error.

```rust
// required field with description
#[witness(description = "blake2b-256 hash of the preimage")]
pub preimage_hash: [u8; 32];  // would need [u8; 32] support added

// optional field
#[witness(required = false)]
pub extra: Vec<u8>,

// required field, no description (default)
pub nonce: u64,
```

---

## Wire format

Fields are encoded sequentially in declaration order, with no outer framing or envelope. The format is identical in both directions — the macro generates the decoder (`from_witness_args`) and the `ckb-idl-client` library generates the encoder for wallets.

- **Scalar types** (`u8`, `u32`, `u64`): read/write N bytes as little-endian.
- **Fixed arrays** (`[u8; N]`): read/write exactly N bytes.
- **Variable bytes** (`Vec<u8>`): 4-byte little-endian length prefix, followed by that many bytes.

Any trailing bytes after all fields are consumed produce `WitnessError::TrailingBytes`.

---

## IDL commitment at deployment

The `idl.json` generated by this macro is the file that gets hashed and appended to the code cell at deployment time:

```
code_cell_data = risc_v_binary || sha256(idl.json)
```

**Important:** the macro regenerates `idl.json` every time `cargo build` or `cargo check` runs, including background runs by rust-analyzer. The JSON key order is stable but the file bytes can vary if the struct definition changes. The deployer must snapshot the exact bytes used when computing the hash — see the `ckb-idl-client` documentation for how the frozen file pattern works.

---

## Constraints

- Only named-field structs are supported. Enums, unions, tuple structs, and unit structs are rejected at compile time.
- The struct must be in a crate with a valid `CARGO_MANIFEST_DIR` (standard for any Cargo build).
- All field types must be in the supported type table above.

---

## Error messages

Compile-time errors are designed to be clear:

```
error: unrecognised type `String` for field `label`;
       supported types are: u8, u32, u64, [u8; 33], [u8; 64], [u8; 65], Vec<u8>
```

```
error: #[derive(CkbWitness)] only supports named-field structs
```

```
error: unknown attribute key `deprecated` in #[witness(...)]; supported keys: required, description
```

---

## Planned extensions

The type registry is intentionally small for v0. Types under consideration for future versions:

| Candidate Rust type  | Candidate IDL type   | Notes                                        |
|----------------------|----------------------|----------------------------------------------|
| `[u8; 32]`           | `hash256`            | For commitment fields, Merkle roots, etc.    |
| `[u8; N]` (general)  | `bytes_fixed_N`      | Any fixed-length byte array                  |
| `u128`               | `uint128`            | 16 bytes, little-endian                      |
| Molecule types       | `molecule_<type>`    | Requires a separate codec attribute          |

The decision on what goes into the type registry should be driven by what real scripts need — not by what is theoretically possible. Scripts that need Molecule encoding today should treat the encoded bytes as `Vec<u8>` and handle decoding internally.

---

## How it fits into the system

```
Script author                          Wallet / tooling
─────────────                          ────────────────
#[derive(CkbWitness)]              →   ckb-idl-client fetches/verifies IDL
  generates idl.json               →   validates witness before tx submission
  generates from_witness_args()    ←   encodes wire bytes per IDL
  
Deployer appends sha256(idl.json)
to code cell data at deployment
```

The macro is the source of truth. Everything downstream — the on-chain commitment, the client decoder, the test vectors — is derived from what the macro generates.

---

## Testing

The macro has property-based tests using `proptest` covering:
- IDL structural invariants (field count, required flag fidelity, description round-trip)
- JSON serialisation stability
- Const token generation for arbitrary valid structs

Run them with:

```bash
cargo test
```

The wire format is independently tested in `ckb-idl-client` via `test-vectors.json`, which serves as the canonical cross-language specification.
