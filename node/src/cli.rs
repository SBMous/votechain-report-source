use clap::Parser;

/// Root of the cli
#[derive(Parser, Debug)]
#[clap(author = "Yarnley, George", version, about)]
pub(crate) struct Cli {
    /// Optional path for overriding the location of the config file
    #[arg(long, default_value = "./config.toml")]
    pub(crate) config: Option<String>,

    /// An addendum to the chain for testing
    #[arg(short, long)]
    pub(crate) chain_postfix: Option<String>,

    /// Hex-encoded private key
    #[arg(long)]
    pub(crate) private_key: Option<String>,

    // /// The number of peers we want to ack the vote cast before closing
    // #[arg(long)]
    // peer_id: Option<String>,

    /// CLI args for integration test scenarios
    /// How many blocks should we append to this chain after a delay?
    #[arg(short, long, default_value_t = 0)]
    pub(crate) test_append: u32,

    /// Which of the available test identities should we use
    #[arg(long)]
    pub(crate) test_identity: Option<u32>,
}
