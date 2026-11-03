mod minimax;

pub(crate) use minimax::transport::MiniMaxTransport;
#[cfg(test)]
pub(crate) use minimax::transport::{MiniMaxRequest, MiniMaxResponse};
pub(crate) use minimax::{call, catalog};
