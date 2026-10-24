//! Shared synthetic builders for integration tests. Invented values only.
#![allow(dead_code)]

/// Synthetic v3.1 ticket with the entry layout of the observed official ticket.
pub fn ticket(name: &[u8], first_six: [u16; 6]) -> Vec<u8> {
    let mut body = vec![];
    let mut put = |kind: u16, value: &[u8]| {
        body.extend(kind.to_be_bytes());
        body.extend((value.len() as u16).to_be_bytes());
        body.extend(value);
    };
    let mut id = [0u8; 32];
    id[..name.len()].copy_from_slice(name);
    let values: [&[u8]; 6] = [
        &[0x11; 20],
        &[0, 0, 1, 0],
        &[0; 8],
        &[0; 8],
        &[0x22; 8],
        &id,
    ];
    for (kind, value) in first_six.into_iter().zip(values) {
        put(kind, value);
    }
    put(8, &[0; 4]);
    put(4, &[0; 4]);
    put(8, &[0; 24]);
    let mut sections = 0x3000u16.to_be_bytes().to_vec();
    sections.extend((body.len() as u16).to_be_bytes());
    sections.extend(body);
    sections.extend([0x30, 0x02, 0, 8, 0, 8, 0, 4, 0, 0, 0, 0]);
    let mut out = 0x3100_0000u32.to_be_bytes().to_vec();
    out.extend((sections.len() as u32).to_be_bytes());
    out.extend(sections);
    out
}

#[allow(dead_code)]
pub fn profile() -> bo2_server::profile::Profile {
    let p: bo2_server::profile::Profile =
        serde_json::from_str(include_str!("../fixtures/profile.json")).unwrap();
    p.validate().unwrap();
    p
}
