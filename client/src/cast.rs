use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Path;
use std::time::Duration;

use async_std::task::block_on;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::SigningKey;
use libp2p::identity::Keypair;
use libp2p::swarm::NetworkBehaviour;
use libp2p::swarm::SwarmEvent;
use libp2p::Multiaddr;
use paillier::{Paillier, KeyGeneration};
use clap::Args;
use libp2p::{gossipsub, noise, tcp, yamux, identify};
use futures::{FutureExt, StreamExt};

use rand::rngs::OsRng;
use vote_lib::{Signed, Ballot};

use crate::config::Cfg;

#[derive(Args, Debug)]
pub(crate) struct CastArgs {
    /// An identifier representing the specific chain they wish to vote on
    #[arg(short, long)]
    issue: String,

    /// The user's vote intent. True = Yes, False = No
    #[arg(short, long)]
    verdict: bool,

    /// The identity the user wishes to sign as
    #[arg(long)]
    id: Option<u32>,

    #[arg(long)]
    peer_port: Option<u32>,

    // / The number of peers we want to ack the vote cast before closing
    // #[arg(long, default_value_t = 4)]
    // required_peers: u32,
}

#[derive(NetworkBehaviour)]
struct NodeBehaviours {
    // Behaviour for PubSub via GossipSub
    gossipsub: gossipsub::Behaviour,
    // mdns: mdns::async_io::Behaviour,
    identify: identify::Behaviour,
}

pub(crate) async fn cast(args: CastArgs, cfg: Cfg) {
    println!("Building Vote Packet");
    
    // Submit using fixed testing key
    let (ek, _dk) = bincode::deserialize::<paillier::Keypair>(&std::fs::read("./temp/trustees.key").unwrap()).unwrap().keys();

    let ballot = Ballot::new(&ek, args.verdict, args.issue);

    let sk = match args.id {
        Some(identity) => {
            let keyfile = format!("./temp/identities/{identity}.der");
            println!("Reading key from file: {keyfile}");
            SigningKey::read_pkcs8_der_file(keyfile).unwrap()
        },
        None => DecodePrivateKey::read_pkcs8_der_file(Path::new(&cfg.secret_key_path)).unwrap(),
    };
    let ballot_signed = Signed::new(&sk, ballot);

    println!("Casting Vote:\n{}, size: {}", ballot_signed, bincode::serialize(&ballot_signed).unwrap().len());

    let peer_port = match args.peer_port {
        Some(port) => port,
        None => 47474,
    };

    send_to_swarm(ballot_signed, cfg, sk, peer_port).await;
}

// -> Result<(), ErrorType>
async fn send_to_swarm(ballot: Signed<Ballot>, cfg: Cfg, sk: SigningKey, peer_port: u32) {
    // let keypair = Keypair::ed25519_from_bytes(&mut sk.to_keypair_bytes()).unwrap();
    // TODO: Update to load from existing identity (Only allow provided identities)
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_async_std()
        .with_tcp(
            tcp::Config::default(),
            // Crypto Primitive for Key Agreement
            noise::Config::new,
            // Multiplexer
            yamux::Config::default,
        )
        // TODO: Replace with proper error handling? What are the failure conditions for this construction
        .unwrap()
        .with_quic()
        .with_behaviour(|key| {
            // To content-address ballot, we can use the associated Public Key
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            // Set a custom gossipsub configuration
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                // TODO: Update Interval to use a config/flag
                .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
                .max_transmit_size(1000000) // Expand maximum transmit size to fit ballots with proofs
                .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
                .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                .build()
                .unwrap(); // Potentially replace with better error handling which maps the err to std::error::Error

            // build a gossipsub network behaviour
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )
            .unwrap();

            // let mdns =
            //     mdns::async_io::Behaviour::new(mdns::Config::default(), key.public().to_peer_id()).unwrap();

            let identify = identify::Behaviour::new(identify::Config::new(
                "/ipfs/id/1.0.0".to_string(),
                key.public(),
            ));
            
            return Ok(NodeBehaviours { gossipsub, identify })
        })
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    
    println!("Listening");
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();

    println!("Topicing");
    // Create a Gossipsub topic
    let topic = gossipsub::IdentTopic::new("ballot-cast");
    // subscribes to our topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();

    println!("Dialing");
    let peer: Multiaddr = format!("/ip4/192.168.1.7/tcp/{}", peer_port).parse().unwrap();
    swarm.dial(peer).unwrap();

    // TODO: Implement a protocol to discover a certain threshold of nodes to publish to
    // Do not publish until we have the required number of nodes
    block_on(async {
        let mut delay = futures_timer::Delay::new(std::time::Duration::from_secs(5)).fuse();
        loop {
            futures::select! {
                event = swarm.next() => {
                    match event.unwrap() {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            tracing::info!(%address, "Listening on address");
                        }
                        event => println!("{event:?}"),
                    }
                }
                _ = delay => {
                    // Likely listening on all interfaces now, thus continuing by breaking the loop.
                    break;
                }
            }
        }
    });
    println!("Done Waiting");


    // let peer_id: PeerId = PeerId::from_str("12D3KooWGqHvzVFxRcs3CwkDRtEfCYwkfDnez2um8PXBv1Cho6Vv").unwrap();
    println!("{:?}", swarm.network_info());
    // swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
    for (peer, topics) in swarm.behaviour_mut().gossipsub.all_peers() {
        println!("{peer}, {topics:?}")
    }

    match swarm.behaviour_mut().gossipsub.publish(topic, bincode::serialize(&ballot).unwrap()) {
        Ok(res) => println!("Succesfully published ballot:\n{res}"),
        Err(e) => println!("Error publishing ballot:\n{e}")
    }

    block_on(async {
        let mut delay = futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse();
        println!("Waiting 15 seconds for vote ACK");
        loop {
            futures::select! {
                event = swarm.next() => {
                    match event.unwrap() {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            tracing::info!(%address, "Listening on address");
                        }
                        event => println!("{event:?}"),
                    }
                }
                _ = delay => {
                    // Likely listening on all interfaces now, thus continuing by breaking the loop.
                    break;
                }
            }
        }
    });
}