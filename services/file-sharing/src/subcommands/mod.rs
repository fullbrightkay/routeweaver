use crate::manifest::BlockId;
use routeweaver_common::PublicKey;
use serde::{Deserialize, Serialize};

mod create_manifest;
pub use create_manifest::create_manifest;
//mod download;
mod serve;
pub use serve::serve;
mod mount;

#[derive(Serialize, Deserialize, Debug)]
pub enum Message {
    QueryBlock { block: BlockId },
    Block { block: BlockId, data: Vec<u8> },
    BlockElsewhere { block: BlockId, location: PublicKey },
    UnknownBlock { block: BlockId },
}
