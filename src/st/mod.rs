mod stt_future;
#[macro_use]
mod wheel;
mod algorithm;
#[macro_use]
mod config;
mod handle;
mod atomic_marker;

pub use handle::StaticHandle;
pub use wheel::{StaticWheelDef, StaticWheel};
pub use stt_future::StaticFuture;
pub use config::{handle_task, FnPtrWrapper};

#[repr(u8)]
#[derive(Copy,Clone,Eq,PartialEq,Hash,Debug)]
pub(crate) enum StopReason{ None,Suspended,Cancelled,Finished,Restart,RestartSuspended /*valid if smn does restart,suspend*/ }


#[derive(Clone, Eq, PartialEq, Hash)]
pub struct StaticParams{
    pub(crate) suspended: bool,
    pub(crate) name: Option<&'static str>,
}


impl StaticParams {

    pub const fn new()->Self{ Self{ name: None, suspended: false} }
    /// Set suspended property.
    pub const fn suspend(self, value: bool) -> Self {
        Self{ suspended: value, ..self }
    }
    /// Set name property to static string slice.
    pub const fn name(mut self, name: &'static str) -> Self {
        Self{ name: Some(name), ..self }
    }
    /// Create default parameters (suspended property set to false) with name property set to
    /// static string slice.
    pub const fn named(name: &'static str) -> Self { Self::new().name(name) }
    /// Create default parameters (name property set to none) with suspended property set to given
    /// value.
    pub const fn suspended(value: bool) -> Self { Self::new().suspend(value) }
}