mod votechain;
mod protocols;
mod cli;
mod trustee;
mod census;

use std::{
    collections::hash_map::DefaultHasher, error::Error, hash::{Hash, Hasher}, sync::Arc, time::Duration
};

use crate::{census::DumbCensus, votechain::{block::Block, chain::Blockchain, config::BlockchainConfig}, gossipsub::TopicHash};
use async_std::{io, net::TcpListener, sync::Mutex};
use bincode::deserialize;
use clap::Parser;
use cli::Cli;
use confique::Config;
use ed25519_dalek::{pkcs8::DecodePrivateKey, SigningKey};
use futures::{select, AsyncBufReadExt, FutureExt, StreamExt};
use libp2p::{
    gossipsub, identify, identity, kad::{self, store::MemoryStore}, mdns, noise, swarm::{behaviour::toggle::Toggle, NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId
};
use local_ip_address::local_ip;
use rand::{distributions::Alphanumeric, rngs::OsRng, Rng};
use tracing::{debug, error, info, level_filters::LevelFilter, span, warn, Level};
use tracing_subscriber::EnvFilter;
use vote_lib::{Ballot, Signed};
use protocols::chain_sync;

static DEFAULT_PORT: u16 = 47474;

#[derive(NetworkBehaviour)]
struct NodeBehaviours {
    // Behaviour for PubSub via GossipSub
    gossipsub: gossipsub::Behaviour,

    // mDNS Node Bootstrapping/Initial Discovery
    mdns: Toggle<mdns::async_io::Behaviour>,

    // Kademlia Service Discovery
    kad: kad::Behaviour<MemoryStore>,

    identify: identify::Behaviour,

    chain_sync: chain_sync::behaviour::Behaviour,
}

impl NodeBehaviours {
    fn new(local_keypair: &identity::Keypair, chain: Arc<Mutex<Blockchain>>) -> Self {
        let local_peer_id = local_keypair.public().to_peer_id();

        // To content-address ballot, we can use the associated Public Key
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        // Build gossipsub behaviour
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            // TODO: Update Interval to use a config/flag
            .heartbeat_interval(Duration::from_secs(10)) // Avoid cluttering the log space
            .max_transmit_size(1000000) // Expand maximum transmit size to fit ballots with proofs
            .validation_mode(gossipsub::ValidationMode::Strict) // Strict validation enforces message signing
            .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
            .build()
            .unwrap(); // TODO: Potentially replace with better error handling which maps the err to std::error::Error
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_keypair.clone()),
            gossipsub_config,
        )
        .unwrap();

        // Discover any nodes on the same private network as this node -> Trusted bootstrapping
        let mdns = Toggle::from(Some(mdns::async_io::Behaviour::new(mdns::Config::default(), local_peer_id).unwrap()));

        // Peer discovery and routing via Kademlia DHT
        let kad = kad::Behaviour::new(local_peer_id, MemoryStore::new(local_peer_id));

        let identify = identify::Behaviour::new(identify::Config::new(
            "/pnyx/id/1.0.0".to_string(),
            local_keypair.public(),
        ));

        let chain_sync = protocols::chain_sync::behaviour::Behaviour::new(chain);

        return Self {
            gossipsub,
            mdns,
            kad,
            identify,
            chain_sync,
        };
    }
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Enable logging at 'INFO' Level by default in dev mode
    #[cfg(debug_assertions)]
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .unwrap(),
        )
        .try_init();

    #[cfg(not(debug_assertions))]
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let args: Cli = cli::Cli::parse();

    // Subscribe to a random issue topic for testing purposes
    let issue_id = match args.chain_postfix {
        Some(postfix) => postfix,
        None => {
            rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(7)
                .map(char::from)
                .collect()
        }
    };

    
    let mut signing_key = match args.test_identity {
        Some(identity) => {
            let keyfile = format!("./temp/identities/{identity}.der");
            info!("Reading key from file: {keyfile}");
            SigningKey::read_pkcs8_der_file(keyfile)?
        },
        None => SigningKey::generate(&mut OsRng),
    };

    let census = DumbCensus::new();

    // Setup Storage
    let chain = Arc::new(Mutex::new(Blockchain::new(BlockchainConfig::builder().load()?, issue_id, &signing_key)?));

    // TODO: Link ed25519 signing curve key into libp2p identity
    // let keypair = Keypair::ed25519_from_bytes(&mut signing_key.to_keypair_bytes()).unwrap();
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_async_std()
        .with_tcp(
            tcp::Config::default(),
            // Crypto Primitive for Key Agreement
            noise::Config::new,
            // Connection Multiplexer
            yamux::Config::default,
        )
        // TODO: Replace with proper error handling? What are the failure conditions for this construction
        .unwrap()
        .with_quic()
        .with_behaviour(|key| NodeBehaviours::new(key, chain.clone()))
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Create a Gossipsub topic
    let vote_topic = gossipsub::IdentTopic::new("ballot-cast");
    // subscribes to our topic
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&vote_topic)
        .unwrap();


    // let local_ip = local_ip().unwrap();
    let local_ip = "0.0.0.0";
    let port: u16 = match TcpListener::bind(("127.0.0.1", DEFAULT_PORT)).await {
        Ok(_) => {
            let mut port: u16 = 0;
            if args.test_identity == Some(1) {
                port = DEFAULT_PORT
            }
            port
        },
        // If the port is already in use, bind to a random available one
        Err(_) => 0
    };

    let our_addr: Multiaddr = format!("/ip4/{}/tcp/{}", local_ip, port).parse()?;
    swarm
        .listen_on(our_addr.clone())
        .unwrap();
    // swarm.listen_on("/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap()).unwrap();

    info!(
        // peer_id = swarm.local_peer_id().to_string(),
        "New Node started as {} on {}",
        swarm.local_peer_id().to_string(),
        our_addr
    );

    let mut stdin_buf = io::BufReader::new(io::stdin()).lines().fuse();
    let mut delay = futures_timer::Delay::new(std::time::Duration::from_secs(5)).fuse();

    let (ek, dk) = bincode::deserialize::<paillier::Keypair>(&std::fs::read("./temp/trustees.key").unwrap()).unwrap().keys();

    // Event Handling Loop
    loop {
        select! {
            // Log key info after a delay for debugging
            _ = delay => {
                info!("Swarm Peers: {:?}", swarm.connected_peers().collect::<Vec<&PeerId>>());
                info!("Gossipsub Peers: {:?}", swarm.behaviour().gossipsub.all_peers().collect::<Vec<(&PeerId, Vec<&TopicHash>)>>());
                info!("Mesh Peers: {:?}", swarm.behaviour().gossipsub.all_mesh_peers().collect::<Vec<&PeerId>>());

                println!("{:?}", chain);

                if swarm.connected_peers().collect::<Vec<&PeerId>>().len() != 0 {
                    let peer_id = swarm.connected_peers().collect::<Vec<&PeerId>>()[0].clone();
                    info!("Forcing Sync with peer {}", peer_id);
    
                    swarm.behaviour_mut().chain_sync.force_sync(peer_id);
                }

                

                let mut guard = chain.lock().await;
                let start_h = guard.get_height();

                info!("Post Sync Height: {}", start_h);


                if args.test_append == 0 {
                    continue;
                }

                for height in start_h..(start_h+args.test_append) {
                    let genesis = guard.get_block(&height)?;
                    info!("{}", genesis.hash());
                    
                    let ballots = vec!(Signed::new(&signing_key, Ballot::new(&ek, true, "test".into())));
                    let block = Block::new(&mut signing_key, &genesis, ballots).unwrap();

                    match guard.append(block) {
                        Ok(_) => {
                            info!("Successfully Appended");
                        },
                        Err(e) => println!("Failed: {}", e)
                    };
                }
            }

            // Enable user input to the console in dev mode for debugging.
            line = stdin_buf.select_next_some() => {
                match line {
                    Ok(data) => {
                        let res = swarm.behaviour_mut().gossipsub.publish(vote_topic.hash(), data.as_bytes());
                        if res.is_err() {
                            error!("Publish error: {:?}", res.err().unwrap());
                        }
                    }
                    Err(_) => {}
                }
            },

            // Event Handling
            event = swarm.select_next_some() => match event {
                // MDNS Behaviours for local node discovery -> Development Streamlining
                SwarmEvent::Behaviour(NodeBehavioursEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, multiaddr) in list {
                        info!("mDNS discovered a new peer: {peer_id}, {multiaddr}");
                        
                        // Dial all discovered nodes to add them to our routing table
                        let _ = swarm.dial(multiaddr);

                        swarm.behaviour_mut().chain_sync.add_explicit_peer(peer_id);

                        // TODO: Investigate if the below is more resource efficient for direct peers
                        // swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(NodeBehavioursEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        info!("mDNS discover peer has expired: {peer_id}");
                        // swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                    }
                },

                // GossipSub Message Recieved Event
                SwarmEvent::Behaviour(NodeBehavioursEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message_id: id,
                    message,
                })) => {
                    // Setup span for logging
                    let id_hex: String = format!("{}", id)[0..8].to_string();
                    let span = span!(Level::INFO, "message", id = id_hex);

                    // Recieve ballot, validate and pool
                    info!(parent: &span, "Recieving Ballot...");
                    let ballot: Signed<Ballot> = match deserialize(&message.data) {
                        Ok(ballot) => ballot,
                        Err(_) => {
                            warn!(parent: &span, "Recieved Invalid Ballot: {}", id);
                            continue;
                        }
                    };

                    if !ballot.signature_valid() || !census.contains_voter(&ballot.signer()) {
                        // TODO: Reduce rep score of invalid caster
                        warn!(parent: &span, "Got message: {id} from peer: {peer_id} with invalid signature");
                        continue;
                    }

                    info!(parent: &span, "Got message: '{ballot}' with id: {id} from peer: {peer_id}");
                    swarm.behaviour_mut().chain_sync.pool_ballot(ballot).await;

                    info!("Attempting Evaluation");
                    match swarm.behaviour().chain_sync.try_resolve(&dk, &ek, &census).await {
                        Some(result) => {
                            if result {
                                info!("Vote Passed Successfully")
                            } else {
                                info!("Vote Failed to Pass")
                            }
                        },
                        None => {
                            info!("Not ready to resolve")
                        }
                    }
                },

                // Kad Events
                // TODO: Expand for better discovery. Maybe should register topics against KAD?
                SwarmEvent::Behaviour(NodeBehavioursEvent::Kad(kad::Event::RoutingUpdated {
                    peer, ..
                })) => {
                    info!("Discovered Route to Peer: '{peer}'");
                },

                SwarmEvent::Behaviour(NodeBehavioursEvent::Identify(identify::Event::Sent { peer_id, .. })) => {
                    debug!("Sent identify info to {peer_id:?}");
                },
                SwarmEvent::Behaviour(NodeBehavioursEvent::Identify(identify::Event::Received { peer_id, info: peer_info })) => {
                    debug!("Received {peer_info:?}");
                    for address in peer_info.listen_addrs {
                        // TODO: Work out if we should actually be storing every address we id
                        swarm.behaviour_mut().kad.add_address(&peer_id, address.clone());
                        // let _ = swarm.dial(address);
                    }
                },

                // Startup Events
                SwarmEvent::NewListenAddr { address, .. } => info!("Local node listening on {address}"),

                #[cfg(debug_assertions)]
                other => debug!("Swarm event: {:?}", other),
                #[cfg(not(debug_assertions))]
                _ => {}
            }
        }
    }
}
