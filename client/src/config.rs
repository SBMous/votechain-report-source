use confique::Config;
// use std::net::SocketAddr;

#[derive(Config)]
pub struct Cfg {
    // Signature Config
    #[config(default = "./temp/identities/default.der")]
    pub secret_key_path: String,

    // pub seed_peers: Option<Vec<SocketAddr>>
}