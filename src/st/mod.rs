mod stt_future;
#[macro_use]
mod pooling;
mod wheel;
mod algorithm;
#[macro_use]
mod config;

#[repr(u8)]
#[derive(Copy,Clone,Eq,PartialEq,Hash,Debug)]
pub(crate) enum StopReason{ None,Suspended,Cancelled,Finished,Restart,/*RestartSuspended*/ }
