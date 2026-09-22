#![no_std]

/// Errors that can occur when deserialising a `#[derive(CkbWitness)]` struct
/// from `WitnessArgs` via `from_witness_args`.
#[derive(Debug)]
pub enum WitnessError {
    /// A CKB syscall failed while loading the witness (e.g. index out of range).
    #[cfg(feature = "ckb-contract")]
    Load(ckb_std::error::SysError),

    /// The `lock` field of `WitnessArgs` was absent (`to_opt()` returned `None`).
    MissingLockField,

    /// A field's slice of the wire buffer was shorter than expected.
    FieldTooShort {
        /// The field name, for diagnostics.
        field: &'static str,
        /// How many bytes were needed.
        expected: usize,
        /// How many bytes were actually available.
        got: usize,
    },

    /// The buffer had unconsumed bytes after all fields were decoded.
    TrailingBytes {
        /// How many bytes were consumed.
        consumed: usize,
        /// Total buffer length.
        total: usize,
    },

    /// The element-count prefix of a `Vec<T>` field was read, but the buffer
    /// did not contain enough bytes to decode all declared elements.
    VecElementsTooShort {
        field: &'static str,
        element_index: usize,
    },

    /// The type ID read from the buffer does not match any known union variant
    UnknownUnionTypeId { field: &'static str, type_id: u32 },
}

#[derive(Debug)]
pub struct FieldSpec {
    pub name: &'static str,
    pub idl_type: &'static str,
    pub required: bool,
    pub description: Option<&'static str>,
}
pub trait WitnessFields {
    fn idl_fields() -> &'static [FieldSpec];
    fn decode_fields(buf: &[u8], cursor: &mut usize) -> Result<Self, WitnessError>
    where
        Self: Sized;
}

pub struct UnionVariantSpec {
    /// The stable 4-byte little-endian type ID declared by `#[witness(tag = N)]`.
    pub type_id: u32,
    /// The variant name as a string (e.g. `"Secp256k1"`).
    pub name: &'static str,
    /// A provider for the IDL fields of the variant's inner type.
    ///
    /// This is a function pointer rather than a slice so a union can expose a
    /// static variant table while each inner type owns its own static field
    /// table.
    pub fields: fn() -> &'static [FieldSpec],
}

pub trait WitnessUnion: Sized {
    fn idl_variants() -> &'static [UnionVariantSpec];
    fn decode_union(buf: &[u8], cursor: &mut usize) -> Result<Self, WitnessError>;
}
