use crate::{
    ff::Field,
    protocol::context::dzkp_validator::{Array256Bit, SegmentEntry},
    secret_sharing::{FieldSimd, Vectorizable},
};

/// Trait for fields compatible with DZKPs
/// Field needs to support conversion to `SegmentEntry`, i.e. `to_segment_entry` which is required by DZKPs
#[allow(dead_code)]
pub trait DZKPCompatibleField<const N: usize = 1>: FieldSimd<N> {
    fn as_segment_entry(array: &<Self as Vectorizable<N>>::Array) -> SegmentEntry<'_>;
}

/// Marker Trait `DZKPBaseField` for fields that can be used as base for DZKP proofs and their verification
/// This is different from trait `DZKPCompatibleField` which is the base for the MPC protocol
pub trait DZKPBaseField: Field {
    type UnverifiedFieldValues;
    fn convert(
        x_left: &Array256Bit,
        x_right: &Array256Bit,
        y_left: &Array256Bit,
        y_right: &Array256Bit,
        prss_left: &Array256Bit,
        prss_right: &Array256Bit,
        z_right: &Array256Bit,
    ) -> Self::UnverifiedFieldValues;
}

// TODO(dm) - implement Basefield for Fp61BitPrime in follow up PR
