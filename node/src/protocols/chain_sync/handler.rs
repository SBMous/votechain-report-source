use std::{collections::VecDeque, sync::Arc, task::Poll};

use async_std::sync::Mutex;
use asynchronous_codec::CborCodecError;
use futures::{future::BoxFuture, FutureExt};
use libp2p::{core::upgrade::ReadyUpgrade, swarm::{handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound}, ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol}, PeerId, Stream, StreamProtocol};
use tracing::info;

use crate::votechain::chain::Blockchain;

use super::{behaviour::Event, protocol::{self, ChainSyncInfo, PROTOCOL_NAME}};


/// Events from `Behaviour` with the information requested by the `Handler`.
#[derive(Debug)]
pub enum InEvent {
    /// Explicitly Trigger the behaviour to check with the associated peer
    ForceSync
}

#[derive(Debug)]
pub enum OutEvent {
    /// Successfully updated our chain to a new longest version
    SyncPointFound(ChainSyncInfo),
    /// Failed to update our chain
    SyncError,
}

type SyncSendFuture = BoxFuture<'static, Result<Stream, CborCodecError>>;
type SyncListenFuture = BoxFuture<'static, Result<Stream, CborCodecError>>;

pub struct Handler {
    /// Store a reference to the chain to enable chain interactions 
    chain: Arc<Mutex<Blockchain>>,

    /// Queue containing actively polled events
    // TODO: Work out why the 'identify' module uses a symmetric 'Either' for the protocol upgrade
    events: VecDeque<ConnectionHandlerEvent<ReadyUpgrade<StreamProtocol>, (), Event>>,

    /// Listener for inbound sync requests
    inbound: Option<SyncListenFuture>,

    /// Listener for progressing outbound sync requests
    outbound: Option<SyncSendFuture>,
}

impl Handler {
    pub fn new(chain: Arc<Mutex<Blockchain>>) -> Self {
        return Self {
            chain: chain,
            events: VecDeque::new(),
            inbound: None,
            outbound: None,
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = InEvent;
    type ToBehaviour = Event;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_NAME), ())
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            // Event triggered when an inbound connection is negotiated, expect a message
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: mut stream,
                ..
            }) => {
                info!("Listening Inbound");
                stream.ignore_for_keep_alive();
                self.inbound = Some(protocol::recv_sync(stream, self.chain.clone()).boxed());
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: stream,
                ..
            }) => {
                info!("Negotiated outbound!");
                // stream.ignore_for_keep_alive();
                self.outbound = Some(protocol::send_sync(stream, self.chain.clone()).boxed());
            }
            ConnectionEvent::DialUpgradeError(_dial_upgrade_error) => {
                // TODO: Return an event that we failed to sync?
            }
            _ => {}
        }
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            InEvent::ForceSync => {
                self.events.push_back(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol: SubstreamProtocol::new(
                    ReadyUpgrade::new(PROTOCOL_NAME), ()
                ) })
            }
        }
    }

    // TODO: Update to TRACE once finalised
    #[tracing::instrument(level = "debug", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p::swarm::ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        // Respond to inbound sync.
        if let Some(fut) = self.inbound.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Pending => {}
                Poll::Ready(Err(e)) => {
                    tracing::debug!("Inbound sync error: {:?}", e);
                    self.inbound = None;
                }
                Poll::Ready(Ok(_)) => {
                    tracing::info!("Answered sync request from peer");
                    self.inbound = None;
                }
            }
        }

        // Continue outbound sync.
        if let Some(fut) = self.outbound.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Pending => {}
                Poll::Ready(Err(e)) => {
                    tracing::debug!("Failed to progress sync. Error: {:?}", e);
                    self.outbound = None;
                }
                Poll::Ready(Ok(_)) => {
                    self.outbound = None;
                }
            }
        }

        return Poll::Pending;
    }
}