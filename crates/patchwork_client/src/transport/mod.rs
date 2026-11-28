//! How a client reaches a Patchwork: a [`Transport`].
//!
//! A transport speaks `patchwork-protocol`. [`RemoteTransport`] reaches a Patchwork over its
//! socket, as the phone does. The desktop holds its `Core` in the same process and calls it
//! directly, through the stores in [`crate::local`], so it has no transport.

// Rust guideline compliant 2026-10-09

mod error;
mod remote;

use patchwork_protocol::Method;
use patchwork_protocol::pushes::Push;
use serde_json::Value;

pub use error::ClientError;
pub use remote::RemoteTransport;

/// What a transport hands each push to, on a thread of its own.
pub type PushSink = Box<dyn Fn(Push) + Send + Sync>;

/// A Patchwork a client calls methods on and takes pushes from.
pub trait Transport: Send + Sync + 'static {
    /// Calls `method` with `params` and waits for its answer's `result`.
    ///
    /// # Errors
    ///
    /// Returns the error the Patchwork answered with, or why it could not be reached.
    fn call_raw(&self, method: &str, params: Value) -> Result<Value, ClientError>;

    /// Hands `sink` every push from now on, for as long as the transport lives.
    fn subscribe(&self, sink: PushSink);
}

impl dyn Transport {
    /// Calls the typed method `M`.
    ///
    /// # Errors
    ///
    /// Returns the error the Patchwork answered with, why it could not be reached, or that its
    /// answer is not `M`'s.
    pub fn call<M: Method>(&self, params: &M::Params) -> Result<M::Output, ClientError> {
        let params = patchwork_protocol::encode_params::<M>(params).map_err(ClientError::Rpc)?;
        let result = self.call_raw(M::NAME, params)?;
        patchwork_protocol::decode_output::<M>(result).map_err(ClientError::Rpc)
    }
}
