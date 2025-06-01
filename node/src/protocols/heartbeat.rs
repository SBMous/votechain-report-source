use serde::{Deserialize, Serialize};
use libp2p::{request_response::{self, ProtocolSupport}, StreamProtocol};


#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    height: u32
}

pub type Behaviour = request_response::cbor::Behaviour::<HeartbeatMessage, HeartbeatMessage>;

// impl Default for Behaviour {
//     fn default() -> Self {
//         return Self::new(
//             [(StreamProtocol::new("/votechain/heartbeat/0.0.0"), ProtocolSupport::Full)],
//             request_response::Config::default()
//         );
//     }
// }

pub fn get_behaviour() -> Behaviour {
    Behaviour::new(
        [(StreamProtocol::new("/votechain/heartbeat/0.0"), ProtocolSupport::Full)],
        request_response::Config::default()
    )
}
