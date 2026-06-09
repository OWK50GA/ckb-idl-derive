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
}
