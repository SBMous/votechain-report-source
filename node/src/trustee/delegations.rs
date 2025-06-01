use std::collections::{HashMap, HashSet};
use ed25519_dalek::VerifyingKey;
use rand::{seq::SliceRandom, Rng};



/// Delegation Graph stores the most simple mapping from delegator -> delegate
///  TODO: Potentially switch the delegations hashmap to a more efficient structure, such as a tree
pub struct DelegationGraph {
    /// Storage backing for delegator -> delegate representation
    delegation_map: HashMap<VerifyingKey, VerifyingKey>,
    // delegations: HashMap<VerifyingKey, VerifyingKey>,
    /// Inverted representation for efficient back-traversal
    representation_adj_list: HashMap<VerifyingKey, Vec<VerifyingKey>>,
}

impl DelegationGraph {
    // Testing Only -> TODO: Turn into method to build a new delegation graph from an iterator returning delegation pairs
    pub fn new(delegation_map: HashMap<VerifyingKey, VerifyingKey>) -> Self {
        let representation_adj_list = Self::get_adj_list(&delegation_map);

        return Self {
            delegation_map,
            representation_adj_list
        }
    }

    /// Fill the delegations graph with a set of randomised delegations
    pub fn random(census: Vec<VerifyingKey>) -> Self {
        let mut delegation_map = HashMap::new();
        
        let mut rng = rand::thread_rng();
        for public_key in &census {
            if rng.gen::<f64>() > 0.1 {
                let target = census.choose(&mut rng).unwrap();
                delegation_map.insert(public_key.clone(), target.clone());
            }
        }

        let representation_adj_list = Self::get_adj_list(&delegation_map);

        return Self {
            delegation_map,
            representation_adj_list
        }
    }

    fn get_adj_list(delegation_map: &HashMap<VerifyingKey, VerifyingKey>) -> HashMap<VerifyingKey, Vec<VerifyingKey>> {
        let mut adj_list: HashMap<VerifyingKey, Vec<VerifyingKey>> = HashMap::new();

        // Maps from the iterator, ignoring all 'error' rows
        delegation_map.iter().for_each(|(delegate_key, representative_key)| {
            adj_list
                .entry(representative_key.clone())
                .and_modify(|list| list.push(delegate_key.clone()))
                .or_insert(Vec::from([delegate_key.clone()]));
        });

        return adj_list
    }


    /// Resolve a hashmap of voter-weight pairs for every voter who actually cast a ballot in this vote
    pub fn generate_weights(&self, voters: &HashSet<VerifyingKey>) -> HashMap<VerifyingKey, u64> {
        let mut weights: HashMap<VerifyingKey, u64> = HashMap::new();

        for voter in voters {
            let power = self.resolve_power(voter.clone(), &voters);
            weights.insert(voter.clone(), power);
        }

        return weights
    }

    pub fn resolve_power(&self, public_key: VerifyingKey, voters: &HashSet<VerifyingKey>) -> u64 {
        tracing::info!("Resolving Power for 0x{}", hex::encode(public_key));
        let mut stack: Vec<VerifyingKey> = Vec::from([public_key]);
        let mut visited: HashSet<VerifyingKey> = HashSet::new();
        let mut accumulator = 0;

        while let Some(next) = stack.pop() {
            // Mark node as visited and update power accumulator
            visited.insert(next);
            accumulator += 1;

            match &self.representation_adj_list.get(&next) {
                Some(children) => {
                    children.iter().for_each(|child| if !visited.contains(child) && !voters.contains(child) {stack.push(*child)})
                }
                None => {}
            };
        }
        return accumulator;
    }
}


#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use ed25519_dalek::{SigningKey, VerifyingKey};
    use rand::rngs::OsRng;

    use super::DelegationGraph;

    fn get_test_census() -> Vec<VerifyingKey> {
        let mut census = Vec::new();
        for _i in 0..6 {
            let sk = SigningKey::generate(&mut OsRng);
            census.push(sk.verifying_key())
        }
        return census
    }

    fn get_test_delegation(census: &Vec<VerifyingKey>) -> DelegationGraph {
        let mut map: HashMap<VerifyingKey, VerifyingKey> = HashMap::new();
        map.insert(census[0], census[3]);
        map.insert(census[1], census[2]);
        map.insert(census[2], census[3]);
        map.insert(census[4], census[5]);

        let graph = DelegationGraph::new(map);
        return graph
    }

    fn get_test_delegation_cyclic(census: &Vec<VerifyingKey>) -> DelegationGraph {
        let mut map: HashMap<VerifyingKey, VerifyingKey> = HashMap::new();
        map.insert(census[0], census[1]);
        map.insert(census[1], census[2]);
        map.insert(census[2], census[3]);
        map.insert(census[3], census[0]);

        let graph = DelegationGraph::new(map);
        return graph
    }

    #[test]
    fn test_power_resolve() {
        let census = get_test_census();
        let graph = get_test_delegation(&census);

        let mut voters = HashSet::new();
        voters.insert(census[3]);
        voters.insert(census[5]);

        assert!(
            graph.resolve_power(census[3], &voters) == 4 && 
            graph.resolve_power(census[5], &voters) == 2
        );
    }

    #[test]
    fn test_all_cast_weight() {
        let census = get_test_census();
        let graph = get_test_delegation(&census);

        let voter_set: HashSet<VerifyingKey> = HashSet::from_iter(census.iter().cloned());
        
        // If everyone casts a vote, every vote should have a weight of 1
        assert!(graph.generate_weights(&voter_set).values().all(|&weight| weight == 1))
    }

    #[test]
    fn test_mixed_cast_weight() {
        let census = get_test_census();
        let graph = get_test_delegation(&census);

        let mut voter_set: HashSet<VerifyingKey> = HashSet::new();
        voter_set.insert(census[3]);
        voter_set.insert(census[5]);

        let mut expected_weights: HashMap<VerifyingKey, u64> = HashMap::new();
        expected_weights.insert(census[3], 4);
        expected_weights.insert(census[5], 2);
        
        assert!(graph.generate_weights(&voter_set) == expected_weights)
    }

    #[test]
    fn test_mixed_cast_weight_extra() {
        let census = get_test_census();
        let graph = get_test_delegation(&census);

        let mut voter_set: HashSet<VerifyingKey> = HashSet::new();
        voter_set.insert(census[3]);
        voter_set.insert(census[5]);
        voter_set.insert(census[1]);

        let mut expected_weights: HashMap<VerifyingKey, u64> = HashMap::new();
        expected_weights.insert(census[3], 3);
        expected_weights.insert(census[5], 2);
        expected_weights.insert(census[1], 1);
        
        assert!(graph.generate_weights(&voter_set) == expected_weights)
    }

    #[test]
    fn test_all_cast_weight_cyclic() {
        let census = get_test_census();
        let graph = get_test_delegation_cyclic(&census);

        let voter_set: HashSet<VerifyingKey> = HashSet::from_iter(census.iter().cloned());

        // If everyone casts a vote, every vote should have a weight of 1
        assert!(graph.generate_weights(&voter_set).values().all(|&weight| weight == 1))
    }

    #[test]
    fn test_mixed_cast_weight_cyclic() {
        let census = get_test_census();
        let graph = get_test_delegation_cyclic(&census);

        let mut voter_set: HashSet<VerifyingKey> = HashSet::new();
        voter_set.insert(census[0]);
        voter_set.insert(census[1]);

        let mut expected_weights: HashMap<VerifyingKey, u64> = HashMap::new();
        expected_weights.insert(census[0], 3);
        expected_weights.insert(census[1], 1);
        
        assert!(graph.generate_weights(&voter_set) == expected_weights)
    }
}
