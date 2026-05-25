# Weekly Progress Report
## Week 3: Bootstrapping ckb-idl-derive Project

This week, I successfully bootstrapped the `ckb-idl-derive` project, a Rust procedural macro crate designed for CKB lock-script authors. The project enables developers to declare witness layouts directly in Rust code and automatically generate machine-readable IDL (Interface Definition Language) artifacts at compile time.

### Project Overview and Design

The core functionality revolves around the `#[derive(CkbWitness)]` macro, which can be applied to named-field structs. At compile time, the macro performs several key operations:

1. **Type Mapping**: It reads each field's Rust type and maps it to a corresponding "blessed" IDL type string. Supported types include:
   - `u8` → `"uint8"`
   - `u32` → `"uint32"`
   - `u64` → `"uint64"`
   - `[u8; 65]` → `"secp256k1_sig"`
   - `[u8; 33]` → `"secp256k1_pubkey"`
   - `[u8; 64]` → `"schnorr_sig"`
   - `Vec<u8>` → `"bytes"`

2. **Metadata Collection**: The macro collects `#[witness(...)]` field attributes to include `required` and `description` metadata for each field.

3. **IDL Generation**: It writes an `idl.json` file to the `$OUT_DIR` directory, describing the witness field array in a structured JSON format.

4. **Runtime Integration**: The macro emits a constant `_CKB_WITNESS_IDL_PATH` that points to the generated IDL file, allowing downstream tooling to locate and utilize the witness layout information.

### Design Constraints and Limitations

- **Struct Requirements**: Only named-field structs are supported; enums, unions, tuple structs, and unit structs are rejected at compile time.
- **Build Context**: The macro requires a Cargo build environment where `OUT_DIR` is set.
- **Current Limitations**:
  - `Vec<u8>` maps to `"bytes"` without length-framing hints.
  - Doc comments are not automatically extracted as descriptions (must use explicit `#[witness(description = "...")]`).
  - Molecule-encoded witness types are not yet supported.

### Usage Example

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

This generates an `idl.json` file with the witness layout, enabling type-safe and structured witness handling in CKB scripts.

The project is structured as a procedural macro crate with no runtime dependencies, making it lightweight and focused on compile-time code generation.

---

### Challenges

**1. Semantic Ambiguity in the Type Registry**

The most significant design challenge is that Rust's type system carries structural information but not semantic intent. A fixed-size byte array like `[u8; 32]` could legitimately represent a Blake2b-256 hash, a Schnorr public key, a lock script hash, or an arbitrary 32-byte payload — the type alone does not tell us which. The current registry sidesteps this by only mapping sizes that have a single unambiguous CKB meaning (`[u8; 65]` for secp256k1 signatures, `[u8; 33]` for compressed secp256k1 public keys, `[u8; 64]` for Schnorr signatures), but this leaves a large class of real-world witness fields — particularly hash fields and custom byte arrays — with no automatic mapping. The developer must either use a supported type or face a compile-time error with no escape hatch. We have not yet settled on the right extensibility mechanism: options under consideration include an explicit `#[witness(type = "blake2b_hash")]` override attribute, a user-extensible registry, or a newtype-wrapper convention. Each approach has different tradeoffs around ergonomics, forward compatibility, and the risk of developers supplying incorrect type strings.

**2. Variable-Length Fields and Length Framing**

`Vec<u8>` maps to the IDL type `"bytes"`, which correctly describes a variable-length byte blob, but the IDL consumer has no way to know how that blob is framed within the witness. CKB witnesses can use length-prefixed encoding, fixed-tail placement, or a "remainder of witness" convention, and the correct interpretation depends on the lock script's own parsing logic. The current design accepts this as a known v1 limitation, but it means the generated IDL is incomplete for any witness that contains more than one variable-length field — the consumer cannot reconstruct the wire layout from the IDL alone. A future `#[witness(layout = "remainder")]` or similar hint attribute is the planned path forward, but the exact attribute surface and the set of layout variants to support are still open questions.

**3. Molecule Integration**

Molecule is the canonical serialisation format for CKB on-chain data, and many real lock scripts use Molecule-encoded structs as witness fields. The current type registry has no concept of Molecule types: a field typed as a Molecule-generated struct (e.g. `WitnessArgs`) is simply unrecognised and causes a compile error. Supporting Molecule would require either a naming convention (map any type whose name appears in a Molecule schema to the schema's type name) or an explicit attribute override. This is deferred to a future version, but it limits the practical applicability of the crate for scripts that already use Molecule today.

**4. Single IDL File Per Crate**

The macro always writes to `$OUT_DIR/idl.json`, which means a crate that defines more than one witness struct will have each invocation overwrite the previous file. The current design implicitly assumes one witness struct per crate, but this constraint is not enforced or documented clearly. If a crate legitimately needs multiple witness layouts (e.g. for different entry points), the output strategy needs to change — either by appending to a shared file, using per-struct filenames, or requiring the developer to aggregate manually. This is an open design question with no agreed resolution yet.

**5. Proc-Macro Testing Constraints**

Proc-macro crates cannot call their own entry points from within `#[cfg(test)]` unit tests — the proc-macro host is not available in that context. This forces the test strategy to split across three layers: unit tests on pure internal functions, `trybuild` integration tests for compile-time error messages, and property-based tests that exercise the IDL construction logic directly by bypassing the macro entry point. While this layered approach is sound, it adds friction: property tests that want to verify end-to-end behaviour (including the emitted `TokenStream` and the written file) require a real `OUT_DIR`, which means spinning up a temporary directory and carefully managing environment variables inside the test process. Getting this harness right without making the tests brittle or environment-dependent is an ongoing effort.
