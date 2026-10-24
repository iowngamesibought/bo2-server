//! Runtime extraction from an identity-checked local executable and reviewed addresses.
use crate::{
    client_build::{self, Build, SUPPORTED_BUILD},
    elf::Elf,
    profile::Profile,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

type Error = Box<dyn std::error::Error>;
const MAX_INPUT: u64 = 32 * 1024 * 1024;

pub struct ClientData {
    pub profile: Profile,
    pub profile_json: Value,
    pub endpoints: Value,
}

impl ClientData {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_INPUT {
            return Err("input must be a regular file no larger than 32 MiB".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_INPUT + 1).read_to_end(&mut bytes)?;
        verify_input(&bytes, &SUPPORTED_BUILD)?;
        let elf = Elf::parse(&bytes)?;
        let profile = client_build::profile(&elf)?;
        profile.validate()?;
        let [authentication, lobby] = client_build::hosts(&elf)?;
        validate_host(&authentication)?;
        validate_host(&lobby)?;
        let endpoints = json!({
            "notice": "Private application DNS names; never publish. No DNS changes were made.",
            "client_label": SUPPORTED_BUILD.label,
            "source_sha256": SUPPORTED_BUILD.input_sha256,
            "transport": "tcp", "port": 3074,
            "hosts": {"authentication": authentication, "lobby": lobby}
        });
        Ok(Self {
            profile_json: serde_json::to_value(&profile)?,
            profile,
            endpoints,
        })
    }
}

fn verify_input(bytes: &[u8], build: &Build) -> Result<(), &'static str> {
    if bytes.len() as u64 != build.input_bytes
        || format!("{:x}", Sha256::digest(bytes)) != build.input_sha256
    {
        return Err("unsupported executable: size or SHA-256 differs from supported build");
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), &'static str> {
    if host.len() > 253
        || !host.contains('.')
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err("invalid extracted hostname");
    }
    Ok(())
}

pub(crate) fn read_token(elf: &Elf<'_>, ranges: &[(u64, usize)]) -> Result<String, &'static str> {
    if ranges.is_empty() || ranges.len() > 127 {
        return Err("invalid range count");
    }
    let mut result = Vec::new();
    for &(address, length) in ranges {
        if length == 0 || length > 127 || result.len() + length > 127 {
            return Err("identifier exceeds extraction bounds");
        }
        result.extend_from_slice(elf.read(address, length)?);
    }
    if !result
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(b))
    {
        return Err("invalid extracted identifier");
    }
    String::from_utf8(result).map_err(|_| "invalid extracted identifier")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_composite_tokens_and_rejects_invalid_references() {
        let bytes = crate::elf::fixture(256);
        let elf = Elf::parse(&bytes).unwrap();
        assert_eq!(
            read_token(&elf, &[(4096, 7), (4103, 6)]).unwrap(),
            "example_token"
        );
        for ranges in [
            vec![],
            vec![(0, 1)],
            vec![(4096, 128)],
            vec![(4096, 0)],
            vec![(u64::MAX, 2)],
            vec![(4096, 16); 8],
        ] {
            assert!(read_token(&elf, &ranges).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[256] = 0;
        assert!(read_token(&Elf::parse(&invalid).unwrap(), &[(4096, 1)]).is_err());
    }
    #[test]
    fn identity_and_endpoint_validation_fail_closed() {
        // SHA-256 of ASCII abc is an independent standard test vector.
        let build = Build {
            label: "synthetic",
            input_bytes: 3,
            input_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        assert!(verify_input(b"abc", &build).is_ok());
        assert!(verify_input(b"abd", &build).is_err());
        assert!(verify_input(b"ab", &build).is_err());
        assert!(validate_host("auth.example").is_ok());
        for host in [
            "",
            "bad",
            "a..example",
            "-a.example",
            "a-.example",
            "a_b.example",
        ] {
            assert!(validate_host(host).is_err());
        }
    }
}
