use confique::Config;

#[derive(Config)]
pub struct BlockchainConfig {
    #[config(default = "./temp/blockchains/solochain")]
    pub path: String
}