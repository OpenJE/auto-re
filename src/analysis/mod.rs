//! Analysis backend abstraction — trait, capabilities, mock, and packet builder.
//!
//! Callers use `crate::analysis::{AnalysisBackend, AnalysisCapability, MockAnalysisBackend,
//! FunctionAnalysisPacket, PacketBuilder, MockPacketBuilder}`.

mod backend;
mod mock;
mod packet;

pub use backend::{AnalysisBackend, AnalysisCapability};
pub use mock::MockAnalysisBackend;
pub use packet::{FunctionAnalysisPacket, MockPacketBuilder, PacketBuilder};
