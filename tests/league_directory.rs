//! Synthetic vectors packed independently with Python struct from the recovered
//! field order. No game bytes or real users/endpoints. Native receiver execution
//! and stock peer acceptance are separate from these codec tests.
use bo2_server::league_directory::{SearchResult, create_id_result};

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn result() -> SearchResult {
    SearchResult {
        // Six invented TEST-NET endpoints and one synthetic trailing byte.
        address: hex(concat!(
            "c6336401017dc6336402027dc6336403037d",
            "c6336404047dc6336405057dc6336406067d01"
        ))
        .try_into()
        .unwrap(),
        directory_id: [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7],
        field_118: 0x1122_3344,
        field_110: 18,
        field_114: 7,
        peer_id: 0x0102_0304_0506_0708,
        peer_key: [
            0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad,
            0xae, 0xaf,
        ],
        fields_13c_through_158: [-3, 1, 122, 0x12_3456, 17, 23, -9, 5],
        field_15c: 1.25,
        fields_160_through_16c: [11, -22, 33, i32::MAX],
    }
}

#[test]
fn create_id_is_an_opaque_blob_not_a_scalar_peer_identifier() {
    assert_eq!(
        create_id_result(&result().directory_id),
        hex("130808000000f0f1f2f3f4f5f6f7")
    );
    // Allocation/validity policy belongs to the future directory authority.
    assert_eq!(
        create_id_result(&[0; 8]),
        hex("1308080000000000000000000000")
    );
}

#[test]
fn search_result_matches_independent_asymmetric_field_vector() {
    let encoded = result().encode().unwrap();
    assert_eq!(encoded.len(), 168);
    assert_eq!(
        encoded,
        hex(concat!(
            "130825000000c6336401017dc6336402027dc6336403037dc6336404047d",
            "c6336405057dc6336406067d01130808000000f0f1f2f3f4f5f6f7",
            "0844332211081200000008070000000a0807060504030201",
            "130810000000a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
            "07fdffffff0701000000077a000000075634120007110000000717000000",
            "07f7ffffff07050000000d0000a03f070b00000007eaffffff072100000007ffffff7f"
        ))
    );
}

#[test]
fn unusable_peer_identifier_and_nonfinite_aggregate_are_rejected() {
    let mut value = result();
    value.peer_id = 0;
    assert!(value.encode().is_err());
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        value = result();
        value.field_15c = invalid;
        assert!(value.encode().is_err());
    }
}

#[test]
fn unresolved_fields_preserve_bit_width_without_invented_business_policy() {
    let mut value = result();
    value.field_118 = u32::MAX;
    value.peer_id = u64::MAX;
    value.fields_13c_through_158[0] = i32::MIN;
    value.field_15c = -2.5;
    let encoded = value.encode().unwrap();
    // Independently derived offsets: address blob43 + ID blob14 = 57.
    assert_eq!(&encoded[57..62], hex("08ffffffff"));
    assert_eq!(&encoded[72..81], hex("0affffffffffffffff"));
    assert_eq!(&encoded[103..108], hex("0700000080"));
    assert_eq!(&encoded[143..148], hex("0d000020c0"));
    assert_eq!(&encoded[49..57], value.directory_id);
}
