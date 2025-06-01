use std::{io, sync::Arc};

use async_std::sync::Mutex;
use asynchronous_codec::{CborCodec, CborCodecError, Framed};
use blake3::Hash;
use futures::{AsyncRead, AsyncWrite, SinkExt, TryStreamExt};
use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};

use crate::votechain::{block::Block, chain::Blockchain, errors};


pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/votechain/sync/0.0");


/// Request allowing a responding node to identify the divergence point
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncRequest {
    index: u32,
    hash: Hash,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChainSyncInfo {
    pub fork_index: u32,

    /// Vector of blocks (Including the shared divergence point block)
    pub block: Block,

    /// Number of blocks still to be sent
    remaining: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SyncResponse {
    Found(ChainSyncInfo),
    NotFound,
}

/// Initiates the sync with a given node
/// 1. Identify fork point
/// 2. Request all blocks post fork
/// 3. Return event to process new block chain
/// 
/// TODO: May need to have additional capabilities for handling large diffs (Incremental read)
/// If a long way behind we may also want to do additional work to validate that the chain we're
/// pulling matches a few randomly selected nodes up to a certain point
pub async fn send_sync<S>(mut stream: S, chain: Arc<Mutex<Blockchain>>) -> Result<S, CborCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tracing::info!("Starting Send Protocol");

    let codec = CborCodec::<SyncRequest, SyncResponse>::new();
    let mut framed_stream = Framed::new(&mut stream, codec);

    let mut height = chain.lock().await.get_height();
    let mut block_buffer: Vec<Block> = Vec::new();

    loop {
        // Firstly send our request
        let _ = framed_stream.send(SyncRequest { index: height, hash: chain.lock().await.get_hash_at(height).unwrap() }).await;
        tracing::info!("Sent Request");

        // Await the response
        match framed_stream.try_next().await?{
            // Peer found the divergence point and sent us the updates
            Some(SyncResponse::Found(result)) => {
                tracing::info!("Peer responded with block. Sync Height: {}, Remaining: {}, Head Hash: {}", result.fork_index, result.remaining, result.block.hash());
                block_buffer.push(result.block);

                if result.remaining == 0 {
                    let mut guard = chain.lock().await;
                    tracing::info!("Sync: Obtained lock");
                    let update_result = guard.try_update_longest(result.fork_index, block_buffer);
                    if update_result.is_err() {
                        tracing::info!("Failed to update: {}", update_result.unwrap())
                    }
                    return Ok(stream)
                }
            },
            // Peer failed to match the provided hash and height, try again
            Some(SyncResponse::NotFound) => {
                // Failed to match genesis block, other chain is invalid
                if height == 1 {
                    tracing::info!("Peer did not match genesis block, assuming invalid");
                    framed_stream.close().await?;
                    return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::Other, "Failed to find valid sync point with peer")));
                }

                tracing::info!("Peer failed to find block at height {}", height);
                height -= 1;
                continue;
            },
            None => {
                tracing::info!("Didn't recieve a response from the peer");
                framed_stream.close().await?;
                return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::TimedOut, "Didn't recieve a response from the peer")));
            }
        };
    }
}


/// Respond to heartbeat requests
pub(crate) async fn recv_sync<S>(mut stream: S, chain: Arc<Mutex<Blockchain>>) -> Result<S, CborCodecError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tracing::info!("Starting Listener Protocol");
    let codec = CborCodec::<SyncResponse, SyncRequest>::new();
    let mut framed_stream = Framed::new(&mut stream, codec);

    loop {
        // Await info from the initaiting peer
        match framed_stream.try_next().await?{
            Some(request) => {
                tracing::info!("Recieved Request");
                let guard = chain.lock().await;
                tracing::info!("Current Height: {}", guard.get_height());

                let hash = match guard.get_hash_at(request.index) {
                    Ok(hash) => hash,
                    Err(e) => {
                        match e {
                            // If BlockNotFound - Their chain is longer, so stop the search
                            // TODO: Initiate backwards sync
                            errors::Error::BlockNotFound(_) => {
                                return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::Other, "Failed to find valid sync point with peer")));
                            },
                            _ => {
                                return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::Other, "Other Error Happened")));
                            }
                        }
                    }
                };

                if hash == request.hash {
                    tracing::info!("Found divergence at index {}", request.index);
                    let blocks = match guard.blocks_from(request.index) {
                        Ok(blocks) => blocks,
                        Err(e) => {
                            tracing::info!("Block Read Error: {}", e);
                            Vec::new()
                        }
                    };

                    tracing::info!("Sending");
                    let mut remaining: u32 = (blocks.len()).try_into().unwrap();
                    for block in blocks {
                        let send_result = framed_stream.send(SyncResponse::Found(ChainSyncInfo {
                            fork_index: request.index,
                            block,
                            remaining: remaining - 1
                        })).await;
                        match send_result {
                            Ok(_) => {
                                remaining -= 1;
                            },
                            Err(e) => { tracing::info!("Sync had an error: {}", e); }
                        };
                    }

                    tracing::info!("Sent Sync Response");
                    return Ok(stream);
                };

                let _ = framed_stream.send(SyncResponse::NotFound).await;

                if request.index == 0 {
                    tracing::info!("Peer did not match genesis block, assuming invalid");
                    return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::Other, "Failed to find valid sync point with peer")));
                }
            },
            None => {
                framed_stream.close().await?;
                return Err(CborCodecError::Io(std::io::Error::new(io::ErrorKind::TimedOut, "Didn't recieve metadata from the peer")));
            }
        };
    }
}
