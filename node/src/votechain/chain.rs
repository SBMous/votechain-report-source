use std::{collections::HashMap, fmt::{Debug, Display}, fs, path::Path};

use blake3::Hash;
use ed25519_dalek::SigningKey;
use heed::{types::{OwnedType, SerdeBincode}, Database, Env, EnvOpenOptions};
use tracing::info;
use vote_lib::{Ballot, Signed};

use super::{block::Block, errors::Error, config::BlockchainConfig};


// TODO: Tune block size to optimize for memory footprint
const BLOCK_SIZE: usize = 2;

// TODO: Make blockchain access methods async & include interior 
// mutexing (Assume that the chain is shared amongst potentially many threads)

struct ChainMetadata {
    pub height: u32,
}

pub struct Blockchain {
    // Primary Chain Storage
    chain_config: BlockchainConfig,
    db_env: Env,
    chain_db: Database<OwnedType<u32>, SerdeBincode<Block>>,
    hash_indexes: HashMap<Hash, u32>,
    metadata: ChainMetadata,

    // Pool of unsubmitted votes
    ballot_pool: Vec<Signed<Ballot>>,

    // Node Private key for adding new blocks
    signing_key: SigningKey
}

impl Blockchain {
    pub fn new(config: BlockchainConfig, issue_id: String, sk: &SigningKey) -> Result<Self, Error> {
        let path = Path::new(&config.path).join(Path::new(&issue_id));

        let _ = fs::create_dir_all(path.clone());
        // let env = EnvOpenOptions::new().open(Path::new(path.parent().unwrap()).join(path.file_name().unwrap())).unwrap();
        let env = EnvOpenOptions::new()
                        .map_size(10 * 1024 * 1024)
                        .open(path)
                        .unwrap();
        let block_data_db: Database<OwnedType<u32>, SerdeBincode<Block>> = env.create_database(None).unwrap();

        if block_data_db.is_empty(&env.read_txn()?)? {
            // Build and insert genesis block
            info!("No blocks found, adding genesis");
            let mut wtxn = env.write_txn()?;
            let genesis = Block::genesis();
            block_data_db.put(&mut wtxn, &1, &genesis)?;
            wtxn.commit()?;

            // Setup mapping from hashes to indexes for easier lookup
            let mut hash_index_map = HashMap::new();
            hash_index_map.insert(genesis.hash(), 1);

            return Ok(Self {
                chain_config: config,
                db_env: env,
                chain_db: block_data_db,
                hash_indexes: hash_index_map,
                metadata: ChainMetadata { height: 1 },
                ballot_pool: Vec::new(),
                signing_key: sk.clone(),
            })
        }

        let mut hash_indexes = HashMap::new();
        let mut block_count = 0;
        block_data_db.iter(&env.read_txn()?)?.for_each(|read_result| {
            match read_result {
                Ok((index, block)) => {
                    info!("Read block {}", index);
                    hash_indexes.insert(block.hash(), index);
                    block_count += 1;
                },
                Err(_) => {}
            }
        });

        return Ok(Self {
            chain_config: config,
            db_env: env,
            chain_db: block_data_db,
            hash_indexes: hash_indexes,
            metadata: ChainMetadata { height: block_count },
            ballot_pool: Vec::new(),
            signing_key: sk.clone(),
        })
    }

    pub fn get_block_from_hash(&self, hash: Hash) -> Result<Block, Error> {
        match self.hash_indexes.get(&hash) {
            Some(index) => { return self.get_block(index); },
            None => {}
        }
        return Err(Error::BlockNotFound(0))
    }

    pub fn get_block(&self, index: &u32) -> Result<Block, Error> {
        let rtxn = self.db_env.read_txn()?;
        match self.chain_db.get(&rtxn, index)? {
            Some(block) => Ok(block),
            None => Err(Error::BlockNotFound(*index))
        }
    }

    pub fn try_get_block(&self, index: &u32) -> Option<Block> {
        match self.get_block(index) {
            Ok(block) => Some(block),
            Err(_) => None
        }
    }

    pub fn get_height(&self) -> u32 {
        return self.metadata.height;
    }

    pub fn get_hash_at(&self, index: u32) -> Result<Hash, Error> {
        return Ok(self.get_block(&index)?.hash())
    }

    pub fn blocks_from(&self, start_index: u32) -> Result<Vec<Block>, Error> {
        let rtxn = self.db_env.read_txn()?;
        let mut blocks = Vec::new();

        for index in start_index..self.metadata.height+1 {
            match self.chain_db.get(&rtxn, &index)? {
                Some(block) => blocks.push(block),
                None => return Err(Error::BlockNotFound(index))
            }
        }

        return Ok(blocks);
    }

    /// Append a new block, return the new height if successful
    pub fn append(&mut self, block: Block) -> Result<(), Error> {
        let head_index = &self.metadata.height;
        let head_block = self.get_block(&self.metadata.height)?;

        if !block.is_valid(&head_block) {
            return Err(Error::InvalidNewBlock);
        }

        // Write new block to db
        let mut wtxn = self.db_env.write_txn()?;
        self.chain_db.put(&mut wtxn, &(head_index+1), &block)?;
        wtxn.commit()?;

        self.metadata.height += 1;

        return Ok(());
    }

    /// Call to update the chain to match the longest known chain from the network
    /// If the provided updates are valid, returns a list of 'lost' votes (Votes not included in the new chain which were on the old one)
    /// Otherwise returns None, indicating that the provided set of blocks was somehow invalid.
    // 
    // TODO: Validate Separately or Inline? Ideally this recieves a stream which
    // continuously yeilds older blocks until either we reach genesis or the alternative
    // chain is deemed invalid (May want some early exit clauses too)
    pub fn try_update_longest(&mut self, fork_index: u32, blocks: Vec<Block>) -> Result<u32, Error> {
        if &blocks[0].hash() != &self.get_block(&fork_index)?.hash() || !is_valid_chain(&blocks) {
            return Err(Error::InvalidNewBlock)
        }

        // Strip back to divergence point, appending lost votes to the ballot pool
        let mut wtxn = self.db_env.write_txn()?;
        let range = fork_index..self.metadata.height;
        info!("Stripping");
        for index in range {
            if let Some(block) = self.chain_db.get(&wtxn, &index)? {
                if self.chain_db.delete(&mut wtxn, &index)? {
                    if let Some(ballots) = block.get_ballots() {
                        for ballot in ballots {
                            // TODO: Verify if ballot (Or a newer ballot from the same caster) is already in the pool
                            self.ballot_pool.push(ballot.clone())
                        }
                    }
                };
            }
        }

        // Iteratively reappend
        let mut index = fork_index;
        info!("Appending");
        for block in &blocks {
            info!("New Height: {}", index);
            self.chain_db.put(&mut wtxn, &index, block)?;
            index += 1;
        }

        wtxn.commit()?;

        info!("Finished Update");

        self.metadata.height = index - 1;

        info!("New Sync Height: {}", self.metadata.height);

        return Ok(index);
    }

    pub fn pool_ballot(&mut self, ballot: Signed<Ballot>) -> Result<(), Error> {
        self.ballot_pool.push(ballot);

        // TODO: Replace with configurable block size & tune
        let pool_size = self.ballot_pool.len();
        if pool_size >= BLOCK_SIZE {
            info!("Appending new block");
            let block_ballots = self.ballot_pool.split_off(pool_size - BLOCK_SIZE);
            let prev = &self.get_block(&self.get_height())?;
            let _ = self.append(Block::new(&mut self.signing_key.clone(), prev, block_ballots)?);
        }

        Ok(())
    }

    pub fn blocks(&self) -> Vec<Block> {
        let mut blocks: Vec<Block> = Vec::new();
        for index in 1..self.metadata.height {
            blocks.push(self.get_block(&index).unwrap())
        }
        return blocks
    }

    pub fn iter(self) -> BlockchainIter {
        return BlockchainIter {
            curr_index: 1,
            chain: self
        }
    }

    pub fn seal(self) {
        todo!("Add a sealing block")
    }
}

impl Display for Blockchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let block = self.get_block(&self.metadata.height).unwrap();
        write!(f, "Height: {}\nHeadHash: {}", self.metadata.height, block.hash())
    }
}

impl Debug for Blockchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let block = self.get_block(&self.metadata.height).unwrap();
        f.debug_struct("Blockchain")
            .field("height", &self.metadata.height)
            .field("max_hash", &block.hash())
            .field("db_path", &self.chain_config.path)
            .finish()
    }
}

pub struct BlockchainIter {
    curr_index: u32,
    chain: Blockchain,
}

impl Iterator for BlockchainIter {
    type Item = Block;

    fn next(&mut self) -> Option<Self::Item> {
        self.curr_index += 1;
        return self.chain.try_get_block(&self.curr_index)
    }
}

/// Validate if a vector of blocks represents a valid sequence
pub fn is_valid_chain(blocks: &Vec<Block>) -> bool {
    if blocks.is_empty() || blocks.len() == 1 {
        return true;
    }

    for pair in blocks.windows(2) {
        if pair[0].hash() != pair[1].previous_hash() {
            return false;
        }
    }

    return true;
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use crate::votechain::block::Block;

    use super::is_valid_chain;

    #[test]
    fn empty_chain_valid() {
        assert!(is_valid_chain(&Vec::new()))
    }

    #[test]
    fn valid_chain_validates() {
        let mut sk = SigningKey::from_bytes(&[0; 32]);
        let genesis = Block::genesis();
        let block1 = Block::new(&mut sk, &genesis, Vec::new()).unwrap();
        let block2 = Block::new(&mut sk, &block1, Vec::new()).unwrap();

        let chain = vec![genesis, block1, block2];

        assert!(is_valid_chain(&chain))
    }

    #[test]
    fn invalid_chain_fails() {
        let mut sk = SigningKey::from_bytes(&[0; 32]);
        let genesis = Block::genesis();
        let block1 = Block::new(&mut sk, &genesis, Vec::new()).unwrap();
        let block2 = Block::new(&mut sk, &genesis, Vec::new()).unwrap();

        let chain = vec![genesis, block1, block2];

        assert!(!is_valid_chain(&chain))
    }
}
