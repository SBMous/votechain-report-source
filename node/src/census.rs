use std::{collections::HashSet, fs};

use ed25519_dalek::{pkcs8::DecodePrivateKey, SigningKey, VerifyingKey};

/// Super dumb file-system based census for testing
/// Pulls a list of Identities from a set of keypair files stored locally in the './temp/identities' directory
pub struct DumbCensus {
    census_keys: HashSet<VerifyingKey>
}

impl DumbCensus {
    pub fn new() -> Self {
        let mut census_keys: HashSet<VerifyingKey> = HashSet::new();
        for file in fs::read_dir("./temp/identities/").unwrap() {
            let signing_key: SigningKey = DecodePrivateKey::read_pkcs8_der_file(file.unwrap().path()).unwrap();
            census_keys.insert(signing_key.verifying_key());
        }

        return Self {
            census_keys
        }
    }

    pub fn from_vec(input: Vec<VerifyingKey>) -> Self {
        let mut census_keys: HashSet<VerifyingKey> = HashSet::new();

        for key in input {
            census_keys.insert(key);
        }

        return Self {
            census_keys
        }
    }

    pub fn as_vec(&self) -> Vec<VerifyingKey> {
        return self.census_keys.clone().into_iter().collect()
    }

    pub fn contains_voter(&self, key: &VerifyingKey) -> bool {
        return self.census_keys.contains(key)
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;

    use super::*;

    #[test]
    fn test_contains_valid() {
        let mut csprng = OsRng;
        let key1 = SigningKey::generate(&mut csprng).verifying_key();
        let key2 = SigningKey::generate(&mut csprng).verifying_key();
        let census = DumbCensus::from_vec(vec![key1, key2]);

        assert!(census.contains_voter(&key1))
    }

    #[test]
    #[should_panic]
    fn test_contains_invalid() {
        let mut csprng = OsRng;
        let key1 = SigningKey::generate(&mut csprng).verifying_key();
        let key2 = SigningKey::generate(&mut csprng).verifying_key();
        let census = DumbCensus::from_vec(vec![key1, key2]);

        let key3 = SigningKey::generate(&mut csprng).verifying_key();
        assert!(census.contains_voter(&key3))
    }
}