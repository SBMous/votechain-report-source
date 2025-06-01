mod config;
mod cast;

use std::path::Path;

use ed25519_dalek::pkcs8::EncodePrivateKey;
use clap::{Parser, Subcommand};
use paillier::{KeyGeneration, Paillier};
use rand::rngs::OsRng;
use ed25519_dalek::SigningKey;
use confique::Config;
use local_ip_address::local_ip;

use serde::Serialize;
use tracing_subscriber::EnvFilter;
use crate::config::Cfg;
use crate::cast::CastArgs;


// CLI Structure
#[derive(Subcommand, Debug)]
enum SubCommand {
    Cast(CastArgs),
    InitKeys,
    Debugging,
}

#[derive(Parser, Debug)]
#[clap(author = "Yarnley, George", version, about)]
struct Cli {
    #[clap(subcommand)]
    cmd: SubCommand,

    // #[arg(long)]
    // config: Option<u32>,
}


// CLI Functionality Handlers
fn generate_keys(cfg: Cfg) {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    std::fs::create_dir_all(Path::new(&cfg.secret_key_path).parent().unwrap()).unwrap();
    signing_key.write_pkcs8_der_file(&cfg.secret_key_path).unwrap();

    println!("Wrote signing key to {}", &cfg.secret_key_path)
}

fn generate_population(cfg: Cfg) {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    std::fs::create_dir_all(Path::new(&cfg.secret_key_path).parent().unwrap()).unwrap();
    for index in 6..20 {
        signing_key.write_pkcs8_der_file(Path::new(&cfg.secret_key_path).parent().unwrap().join(index.to_string() + ".der")).unwrap();
    }
    
    println!("Wrote signing key to {}", &cfg.secret_key_path)
}

fn generate_trustee_keys() {
    let kp = Paillier::keypair();
    let _ = std::fs::write("./temp/trustees.key", bincode::serialize(&kp).unwrap());
}


#[async_std::main]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let args = Cli::parse();
    let cfg = Cfg::builder()
        .env()
        .file("./config.toml")
        .load()
        .unwrap();

    let my_local_ip = local_ip();
    
    if let Ok(my_local_ip) = my_local_ip {
        println!("This is my local IP address: {:?}", my_local_ip);
    } else {
        println!("Error getting local IP: {:?}", my_local_ip);
    }

    match args.cmd {
        SubCommand::Cast(cast_args) => cast::cast(cast_args, cfg).await,
        SubCommand::InitKeys => {
            println!("Initialising Keys");
            generate_keys(cfg);
        },
        SubCommand::Debugging => {
            // generate_trustee_keys()
        }
    }
}
