use crate::noise::PrivateKey;
use routeweaver_common::{Peer, Protocol, PublicKey};
use serde::Deserialize;
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use std::collections::{HashMap, HashSet};

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct Keys {
    #[serde_as(as = "DisplayFromStr")]
    pub public: PublicKey,
    #[serde_as(as = "DisplayFromStr")]
    pub private: PrivateKey,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub routing_only: bool,
    #[serde(default)]
    /// Makes the server not try to give peers its public key
    pub anonymous: bool,
    pub keys: Option<Keys>,
    #[serde(default)]
    #[serde_as(as = "HashSet<DisplayFromStr>")]
    pub initial_peers: HashSet<Peer>,
    #[serde(default)]
    #[serde_as(as = "HashSet<DisplayFromStr>")]
    pub initial_denied_peers: HashSet<Peer>,
    #[serde(default)]
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub transport_config: HashMap<Protocol, toml::Value>,
    #[serde(default)]
    pub discovery_config: HashMap<String, toml::Value>,
}
