use routeweaver_common::{ApplicationId, ConnectionId, Peer};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    /// Requests that a remote node gives us its "best peers"
    RequestPeerSuggestion,
    /// A list of what this node considers to be its best peers
    PeerSuggestion {
        peers: HashSet<Peer>,
    },
    /// Requests that a remote node opens a connection with us over said application
    RequestConnection {
        application: ApplicationId,
    },
    /// Last connection was accepted
    ConnectionAccepted {
        application: ApplicationId,
        connection_id: ConnectionId,
    },
    /// Last connection was denied
    ConnectionDenied {
        application: ApplicationId,
    },
    ConnectionHeartbeat {
        connection_id: ConnectionId,
    },
    ConnectionClose {
        connection_id: ConnectionId,
    },
    ConnectionData {
        connection_id: ConnectionId,
        data: Zeroizing<Vec<u8>>,
    },
}
