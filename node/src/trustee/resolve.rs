// Functions for transitioning from a chain of votes to a vote result

use std::{collections::{HashMap, HashSet}, ops::Deref};

use async_std::sync::MutexGuard;
use curv::BigInt;
use ed25519_dalek::VerifyingKey;
use paillier::{Decrypt, DecryptionKey, EncodedCiphertext, Encrypt, EncryptionKey, Paillier, RawPlaintext};
use vote_lib::{Ballot, Signed};

use crate::votechain::chain::Blockchain;

use super::delegations::DelegationGraph;

struct WeightedVote {
    ballot: Ballot,
    weight: u64,
}

pub fn generate_vote_result(dk: &DecryptionKey, ek: &EncryptionKey, chain: &Blockchain, delegations: DelegationGraph) -> bool {
    // Generate a voter -> weighted vote packet hashmap for every voter
    // Only includes entries for individuals who actually cast a vote (Delegators are excluded)
    let mut weighted_votes: HashMap<VerifyingKey, Ballot> = HashMap::new();
    let mut voter_set: HashSet<VerifyingKey> = HashSet::new();

    // Extract the most recent ballot for each voter & create a hashset of all PKs which cast a vote
    for block in chain.blocks() {
        if let Some(ballots) = block.get_ballots() {
            for ballot in ballots {
                voter_set.insert(ballot.signer());
                weighted_votes
                    .entry(ballot.signer())
                    .and_modify(|current_ballot| {
                        if ballot.timestamp() > current_ballot.timestamp() {
                            *current_ballot = ballot.deref().clone()
                        }
                    })
                    .or_insert(
                            ballot.deref().clone()
                    );
            }
        }
    }

    // Temporarily polyfill random delegations
    // let delegations = DelegationGraph::random(voter_set.clone().into_iter().collect());

    let weight_map: HashMap<VerifyingKey, u64> = delegations.generate_weights(&voter_set);

    // Weight all vote packets
    for (voter, weight) in weight_map {
        weighted_votes.entry(voter).and_modify(|ballot| {
            ballot.weight(&ek, weight);
        });
    }

    // Sum all votes homomorphically
    let all_ballots: Vec<&Ballot> = weighted_votes.values().collect();

    let ptxt = RawPlaintext::from(BigInt::from(0));
    let mut result_for = Paillier::encrypt(ek, ptxt.clone());
    let mut result_against = Paillier::encrypt(ek, ptxt);

    for ballot in all_ballots {
        (result_for, result_against) = ballot.sum(&ek, result_for, result_against)
    }

    // Temporary: Decrypt and compare plaintexts
    // TODO: Switch out with protocol as described by Ordinos
    let vote_for: BigInt = Paillier::decrypt(dk, result_for).into();
    let vote_against: BigInt = Paillier::decrypt(dk, result_against).into();

    return vote_for > vote_against
}

#[cfg(test)]
mod tests {
    use paillier::{Paillier, KeyGeneration};

    fn test_resolve() {
        let (ek, dk) = Paillier::keypair().keys();

        assert!(true)
    }
}
