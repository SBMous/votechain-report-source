use std::{io, time::{Duration, Instant}};

use asynchronous_codec::{CborCodec, CborCodecError, Framed};
use futures::{AsyncRead, AsyncWrite, SinkExt, TryStreamExt};
use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use tracing::trace;

use crate::chain::chain::Blockchain;


pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/votechain/0.0.0");


// TODO: Derive a message format for an empty 'ack'
#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    height: u32
}

/// Heartbeat function, periodically checks if we are synchronised with our neighbours
// TODO: Switch Error out for composed behaviour error type
pub(crate) async fn send_heartbeat<S>(mut stream: S, chain: Blockchain) -> Result<(u32, Duration), CborCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let timer = Instant::now();
    // TODO: Extract into lazy static?
    let codec = CborCodec::<HeartbeatMessage, HeartbeatMessage>::new();

    trace!("Sending chain heartbeat");
    let mut framed_stream = Framed::new(&mut stream, codec);
    framed_stream.send(HeartbeatMessage { height: chain.get_height()}).await?;

    // Consider returning the height to the handler instead of creating events in protocol
    match framed_stream.try_next().await?{
        Some(response) => {
            trace!("Peer responded with height {}", response.height);
            framed_stream.close().await?;
            return Ok((response.height, timer.elapsed()))
        },
        None => {
            framed_stream.close().await?;
            return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::TimedOut, "Didn't recieve a response from the peer")));
        }
    };
}


/// Respond to heartbeat requests
pub(crate) async fn recv_heartbeat<S>(mut stream: S, chain: Blockchain) -> Result<u32, CborCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // TODO: Respond with our height

    let codec = CborCodec::<HeartbeatMessage, HeartbeatMessage>::new();
    let mut framed_stream = Framed::new(&mut stream, codec);

    match framed_stream.try_next().await? {
        Some(request) => {
            trace!("Peer sent height {}, responding with {}", request.height, chain.get_height());
            framed_stream.send(HeartbeatMessage { height: chain.get_height()}).await?;
            return Ok(request.height);
        }
        None => {
            framed_stream.close().await?;
            return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::TimedOut, "Never recieved a heartbeat")));
        }
    };
}


/// Initiates the sync with a given node
/// 1. Identify fork point
/// 2. Request all blocks post fork
/// 3. Return event to process new block chain
/// 
/// TODO: May need to have additional capabilities for handling large diffs (Incremental read)
/// If a long way behind we may also want to do additional work to validate that the chain we're
/// pulling matches a few randomly selected nodes up to a certain point
pub async fn send_sync<S>(mut stream: S) -> io::Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // TODO: Initiate and follow sync protocol
    return Ok((stream, Duration::from_micros(0)))
}


/// Respond to heartbeat requests
pub(crate) async fn recv_sync<S>(mut stream: S) -> io::Result<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // TODO: Respond to sync protocol
    
    Ok(stream)
}
