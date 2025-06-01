use std::{path::Path, time::{SystemTime, UNIX_EPOCH}};

use ed25519_dalek::{ed25519::signature::SignerMut, pkcs8::DecodePrivateKey, Signature, SigningKey, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use vote_lib::{Ballot, Signed};
use blake3::{Hasher, Hash};

use super::errors::Error;

// TODO: Breakout block components into different tables and store separately
#[derive(Debug, Serialize, Deserialize, Clone)]
struct BlockMeta {
    timestamp: u128,
    previous_hash: Hash,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum BlockData {
    Genesis(String),
    Ballots(Vec<Signed<Ballot>>),
    Seal(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Block {
    /// Local time on the node when the block was created
    timestamp: u128,
    /// Hash of the previous block in the chain for chain integrity
    previous_hash: Hash,
    /// Public Key of the signer
    signatory: VerifyingKey,
    /// Sign the previous hash to validate the signatory claim
    signature: Signature,
    /// The contents of the block
    data: BlockData,
    /// Random value limiting addition speed
    nonce: [u8; 8]
}

impl Block {
    pub fn new(sk: &mut SigningKey, prev: &Block, data: Vec<Signed<Ballot>>) -> Result<Self, Error> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time Moved Backwards")
            .as_millis();

        if timestamp < prev.timestamp {
            // TODO: Throw an error (Maybe we validate prev on addition to ensure this never happens? Clock resync could mess with it)
        }

        let previous_hash = prev.hash();
        let (nonce, signature) = Block::proof_of_work(sk, &previous_hash);

        return Ok(Self {
            timestamp,
            previous_hash,
            signatory: sk.verifying_key(),
            signature,
            data: BlockData::Ballots(data),
            nonce,
        });
    }

    pub fn seal(sk: &mut SigningKey, prev: &Block) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time Moved Backwards")
            .as_millis();
        let previous_hash = prev.hash();
        let (nonce, signature) = Block::proof_of_work(sk, &previous_hash);
        let data = String::new();

        return Self {
            timestamp,
            previous_hash,
            signatory: sk.verifying_key(),
            signature,
            data: BlockData::Seal(data),
            nonce
        };
    }

    pub fn genesis() -> Self {
        // TODO: Find better way of timestamping genesis blocks, such that they are easily replicated across nodes
        // let timestamp = SystemTime::now()
        //     .duration_since(UNIX_EPOCH)
        //     .expect("Time Moved Backwards")
        //     .as_millis();
        let timestamp = 0;
        let previous_hash: Hash = [0;32].into();
        let mut sk: SigningKey = DecodePrivateKey::read_pkcs8_der_file(Path::new("./temp/identities/default.der")).unwrap();
        let (nonce, signature) = Block::proof_of_work(&mut sk, &previous_hash);
        let data = String::new();

        return Self {
            timestamp,
            previous_hash,
            signatory: sk.verifying_key(),
            signature,
            data: BlockData::Genesis(data),
            nonce,
        };
    }

    pub fn hash(&self) -> Hash {
        let mut hasher = Hasher::new();

        hasher.update(&self.timestamp.to_be_bytes());
        hasher.update(self.previous_hash.as_bytes());
        match &self.data {
            BlockData::Ballots(ballots) => {
                for signed_ballot in ballots {
                    hasher.update(&bincode::serialize(signed_ballot).unwrap());
                }
            },
            _ => {}
        }
        
        
        return hasher.finalize().into();
    }

    pub fn is_valid(&self, prev: &Block) -> bool {
        if self.previous_hash != prev.hash() {
            return false;
        }

        return true;
    }

    // Simple proof of work calculation
    // Iterates to find a Nonce value which results in a signature with
    // it's first 4 bits all zeroes
    fn proof_of_work(sk: &mut SigningKey, hash: &Hash) -> ([u8; 8], Signature) {
        let mut rng = rand::thread_rng();
        let mut nonce: [u8; 8] = [0; 8];
        loop {
            rng.fill_bytes(&mut nonce);
            let mut adjusted_hash = *(hash.clone().as_bytes());
            for i in 0..8 {
                adjusted_hash[i] = adjusted_hash[i] ^ nonce[i];
            }
    
            let sig = sk.sign(&adjusted_hash);
            let bytes = sig.to_bytes();
            if bytes[0] == 0 && bytes[1] & 240 == 0 {
                return (nonce, sig)
            }
        }
    }

    pub fn previous_hash(&self) -> Hash {
        return self.previous_hash.clone();
    }

    pub fn get_ballots(&self) -> Option<&Vec<Signed<Ballot>>> {
        match &self.data {
            BlockData::Ballots(ballots) => return Some(ballots),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;

    use super::*;

    #[test]
    fn validates_correctly() {
        let mut sk = SigningKey::generate(&mut OsRng);
        let initial = Block::genesis();
        let ballots: Vec<Signed<Ballot>> = Vec::new();
        
        let block = Block::new(&mut sk, &initial, ballots).unwrap();

        assert!(block.is_valid(&initial))
    }

    fn test_proof_of_work() {

    }
}
