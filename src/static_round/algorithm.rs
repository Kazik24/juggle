use std::ops::Index;
use crate::static_round::stt_future::StaticFuture;
use std::cell::Cell;
use crate::utils::AtomicWakerRegistry;
use crate::round::algorithm::TaskKey;
use crate::round::Ucw;
use crate::round::stat::StopReason;


const N:usize = 2;
pub(crate) struct StaticAlgorithm{
    registry: Ucw<[StaticFuture;N]>,
    last_waker: AtomicWakerRegistry,
    current: Cell<Option<TaskKey>>,
    suspended_count: Cell<usize>,
}

impl StaticAlgorithm{

    pub(crate) const fn from_raw_config(conf: [StaticFuture;N])->Self{
        Self{
            registry: Ucw::new(conf),
            last_waker: AtomicWakerRegistry::empty(),
            current: Cell::new(None),
            suspended_count: Cell::new(0),
        }
    }
    pub(crate) fn init(&'static self){ //create all self-refs
        let mut suspended = 0;
        for task in self.registry.borrow_mut().iter_mut() {
            if task.get_stop_reason() == StopReason::Suspended {
                suspended += 1;
            }
            task.init(&self.last_waker);
        }
        self.suspended_count.set(suspended);
    }

}