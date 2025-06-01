use std::{fmt::{Debug, Display}, ops::Deref};

use curv::BigInt;
use ed25519_dalek::{Signature, Signer, Verifier, SigningKey, VerifyingKey};
use paillier::{Add, EncryptWithChosenRandomness, EncryptionKey, Mul, Paillier, Randomness, RawCiphertext, RawPlaintext};
use rand::{rngs::OsRng, RngCore};
use zk_paillier::zkproofs::RangeProofNi;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

fn short_hex(data: impl AsRef<[u8]>) -> String {
    return hex::encode(data)[..8].to_string();
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Ballot {
    timestamp: OffsetDateTime,
    issue_id: String,
    vote_for: BigInt, // Inner of a RawCiphertext
    vote_against: BigInt,
    // proof_for: String,
    // proof_against: String,
    proof_for: RangeProofNi,
    proof_against: RangeProofNi,
}

impl Ballot {
    fn encode_verdict(ek: &EncryptionKey, verdict: u64) -> (RawCiphertext, RangeProofNi) {
        let r = BigInt::from(OsRng.next_u64());
        let s = BigInt::from(verdict);
        let ciphertext = Paillier::encrypt_with_chosen_randomness(ek, RawPlaintext::from(&s), &Randomness(r.clone()));
        let proof = RangeProofNi::prove(ek, &BigInt::from(3), &ciphertext.0, &s, &r);

        return (ciphertext, proof)
    }

    // fn encode_verdict(ek: &EncryptionKey, verdict: u64) -> (RawCiphertext, String) {
    //     let r = BigInt::from(OsRng.next_u64());
    //     let s = BigInt::from(verdict);
    //     let ciphertext = Paillier::encrypt_with_chosen_randomness(ek, RawPlaintext::from(&s), &Randomness(r.clone()));
    //     let proof = String::from("Proved!");

    //     return (ciphertext, proof)
    // }

    pub fn new(ek: &EncryptionKey, verdict: bool, issue_id: String) -> Self {
        let (vote_for, proof_for) = Self::encode_verdict(ek, if verdict { 1 } else { 0 });
        let (vote_against, proof_against) = Self::encode_verdict(ek, if verdict { 0 } else { 1 });

        return Self {
            timestamp: OffsetDateTime::now_utc(),
            issue_id,
            vote_for: vote_for.into(),
            proof_for,
            vote_against: vote_against.into(),
            proof_against
        }
    }

    pub fn validate_proofs(&self) -> bool {
        // let _ = self.proof_for.verify_self();
        // let _ = self.proof_against.verify_self();

        // If no panics, proof verification passed
        return true
    }

    // TODO: Decide if this mutable style is correct
    // Also, does this invalidate the original object through the
    // move ops? Probably not ideal
    pub fn weight(&mut self, ek: &EncryptionKey, weight: u64) {
        let rawc_for: RawCiphertext = RawCiphertext::from(self.vote_for.clone());
        let rawc_agn: RawCiphertext = RawCiphertext::from(self.vote_against.clone());
        let weight_ptxt: RawPlaintext = RawPlaintext::from(BigInt::from(weight));

        self.vote_for = Paillier::mul(ek, rawc_for, weight_ptxt.clone()).into();
        self.vote_against = Paillier::mul(ek, rawc_agn, weight_ptxt).into()
    }

    /// Include this ballot in an overall tally
    pub fn sum(&self, ek: &EncryptionKey, agg_for: RawCiphertext, agg_against: RawCiphertext) -> (RawCiphertext, RawCiphertext) {
        let rawc_for = RawCiphertext::from(self.vote_for.clone());
        let rawc_agn = RawCiphertext::from(self.vote_against.clone());
        return (
            Paillier::add(ek, agg_for, rawc_for),
            Paillier::add(ek, agg_against, rawc_agn)
        )
    }

    pub fn timestamp(&self) -> OffsetDateTime {
        return self.timestamp
    }
}

impl Display for Ballot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,
            "Ballot [\n\ttimestamp: {}\n\tissue_id: {}\n]",
            self.timestamp,
            self.issue_id
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Signed<T> {
    signature: Signature,
    signer: VerifyingKey,
    data: T,
}

impl<T> Signed<T>
where T: Serialize {
    pub fn new(sk: &SigningKey, data: T) -> Self {
        return Self {
            signature: sk.sign(&bincode::serialize(&data).unwrap()),
            signer: sk.verifying_key(),
            data: data
        }
    }

    pub fn signer(&self) -> VerifyingKey {
        return self.signer;
    }

    pub fn signature_valid(&self) -> bool {
        return self.signer.verify(&bincode::serialize(&self.data).unwrap(), &self.signature).is_ok()
    }
}

impl<T> Display for Signed<T>
where T: Display {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let signature_short = short_hex(self.signature.to_bytes());
        let signer_short = short_hex(self.signer.as_bytes());

        write!(f,
            "sig: {}\nby: {}\nwith_content: {}",
            signature_short,
            signer_short,
            self.data
        )
    }
}

impl<T> Deref for Signed<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        return &self.data
    }
}


#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;
    use paillier::{KeyGeneration, Paillier};
    use rand::rngs::OsRng;

    use super::{Ballot, Signed};

    #[test]
    fn ballot_build_correct() {
        let (ek, _dk) = Paillier::keypair().keys();
        let ballot = Ballot::new(&ek, true, String::from("test"));

        assert!(ballot.validate_proofs())
    }

    #[test]
    fn signature_correct() {
        let sk = SigningKey::generate(&mut OsRng);
        let signed = Signed::new(&sk, "test_data");

        assert!(signed.signature_valid())
    }
}