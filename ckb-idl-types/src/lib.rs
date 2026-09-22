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
    UnknownUnionTypeId {
        field: &'static str,
        type_id: u32,
    },
}

#[derive(Debug)]
pub struct FieldSpec {
    pub name: &'static str,
    pub idl_type: &'static str,
    pub required: bool,
    pub description: Option<&'static str>,
}

/// A field description that preserves the complete wire structure for host-side
/// LS-IDL export. This stays allocation-free so derives can emit it in CKB
/// `no_std` contracts.
pub struct FieldSchema {
    pub name: &'static str,
    pub required: bool,
    pub description: Option<&'static str>,
    pub semantic_type: Option<&'static str>,
    pub wire_type: fn() -> &'static TypeSchema,
}

pub struct StructSchema {
    pub fields: &'static [FieldSchema],
}

pub enum TypeSchema {
    Uint { bits: u16 },
    FixedBytes { length: usize },
    Bytes,
    Optional { inner: fn() -> &'static TypeSchema },
    Vector {
        element: fn() -> &'static TypeSchema,
        count_prefix_bits: u8,
    },
    Struct { schema: fn() -> &'static StructSchema },
    Union { schema: fn() -> &'static UnionSchema },
}

pub struct UnionVariantSchema {
    pub tag: u32,
    pub name: &'static str,
    pub schema: fn() -> &'static StructSchema,
}

pub struct UnionSchema {
    pub variants: &'static [UnionVariantSchema],
}

pub trait WitnessSchema {
    fn schema() -> &'static StructSchema;
}

pub trait WitnessFields: WitnessSchema {
    /// Exposes the supertrait schema through the same trait bound used for
    /// field decoding, keeping generated diagnostics focused on one contract.
    fn field_schema() -> &'static StructSchema {
        <Self as WitnessSchema>::schema()
    }

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
    fn schema() -> &'static UnionSchema;
    fn idl_variants() -> &'static [UnionVariantSpec];
    fn decode_union(buf: &[u8], cursor: &mut usize) -> Result<Self, WitnessError>;
}
