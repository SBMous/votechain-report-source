use std::{collections::{HashMap, HashSet, VecDeque}, sync::Arc, task::Poll, time::Duration};

use async_std::sync::Mutex;
use futures::StreamExt;
use futures_ticker::Ticker;
use libp2p::{swarm::{behaviour::ConnectionEstablished, ConnectionClosed, FromSwarm, NetworkBehaviour, NotifyHandler, THandlerInEvent, ToSwarm}, PeerId};
use paillier::{DecryptionKey, EncryptionKey};
use tracing::info;
use vote_lib::{Ballot, Signed};
use rand::seq::SliceRandom;

use crate::{census::DumbCensus, trustee, votechain::chain::Blockchain};

use super::handler::{self, Handler};

#[derive(Debug)]
pub enum Event {
    ResolveReady,

    // /// Began a sync attempt with a peer
    // SyncInitiated(PeerId),
    // /// Successfully updated our chain to a new longest version
    // SyncCompleted(PeerId, u32),
    // /// Failed to update our chain
    // SyncError,


    // Old Heartbeat Events
    // /// Heartbeat event sent out to the addressed remote
    // HeartbeatSent(PeerId),
    // /// Heartbeat event returning the discovered height & id of the remote
    // HeartbeatReceived(PeerId, u32),
    // /// Failed to recieve a response from the remote
    // HeartbeatError,
}

pub struct Behaviour {
    /// Store a reference to the node blockchain enabling chain interactions 
    chain: Arc<Mutex<Blockchain>>,

    /// Queue of events awaiting processing
    events: VecDeque<ToSwarm<Event, handler::InEvent>>,

    // / Hashmap of connected Peers
    // connected: HashMap<PeerId, HashMap<ConnectionId, Multiaddr>>,

    /// List of Known Peers used for syncing
    sync_peers: HashSet<PeerId>,

    heartbeat: Ticker,
}

impl Behaviour {
    pub fn new(chain: Arc<Mutex<Blockchain>>) -> Self {
        return Self {
            chain: chain,
            events: VecDeque::new(),
            sync_peers: HashSet::new(),
            // TODO: Make heartbeat time configurable
            heartbeat: Ticker::new(Duration::from_secs(20))
        }
    }

    pub fn force_sync(&mut self, peer_id: PeerId) {
        self.events.push_back(ToSwarm::NotifyHandler {
            peer_id,
            event: handler::InEvent::ForceSync,
            handler: NotifyHandler::Any,
        });
    }

    pub async fn pool_ballot(&mut self, ballot: Signed<Ballot>) {
        let _ = self.chain.lock().await.pool_ballot(ballot);
    }

    pub async fn try_resolve(&self, dk: &DecryptionKey, ek: &EncryptionKey, census: &DumbCensus) -> Option<bool> {
        let guard = self.chain.lock().await;
        if guard.get_height() > 4 {
            return Some(trustee::resolve::generate_vote_result(&dk, &ek, &guard, trustee::delegations::DelegationGraph::random(census.as_vec())));
        }
        return None
    }

    pub fn heartbeat(&mut self) {
        // Select a random peer id
        // TODO: This cloning approach seems stupid, definitely a better way must exist to pull a cloned random value from a hashset
        // Alternatively, investigate if we can make the sync method take a reference
        let mut peers_vec: Vec<PeerId> = Vec::new();
        for peer in self.sync_peers.iter() {
            peers_vec.push(peer.clone());
        };

        if let Some(peer_id) = peers_vec.choose(&mut rand::thread_rng()) {
            info!("HEARTBEAT: Syncing with {}", peer_id);
            self.force_sync(*peer_id);
        } else {
            info!("HEARTBEAT: No peers to sync with");
        }
    }

    pub fn add_explicit_peer(&mut self, peer_id: PeerId) {
        self.sync_peers.insert(peer_id);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _local_addr: &libp2p::Multiaddr,
        _remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Handler::new(self.chain.clone()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _addr: &libp2p::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Handler::new(self.chain.clone()))
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p::PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        // TODO: Replace with mapping from handler event type to behaviour event
        // self.events.push_back(Event::SyncError)
    }

    // TODO: Update to TRACE once finalised
    #[tracing::instrument(level = "debug", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(&mut self, cx: &mut std::task::Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_back() {
            Poll::Ready(event)
        } else {
            if let Poll::Ready(Some(_)) = self.heartbeat.poll_next_unpin(cx) {
                self.heartbeat();
            }

            Poll::Pending
        }
    }

    // TODO: Impl logic for triggering a sync based on swarm behaviours?
    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                ..
            }) => {
                info!("SYNC: Connected to new node");
                self.sync_peers.insert(peer_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                ..
            }) => {
                self.sync_peers.remove(&peer_id);
            }
            _ => {}
        }
    }
}
