use core::sync::atomic::*;
use core::sync::atomic::Ordering::*;
use core::cell::UnsafeCell;
use core::mem::ManuallyDrop;
use core::task::Waker;
use core::convert::identity;

pub(crate) struct AtomicMarkerOption<T>{
    mark: AtomicUsize,
    value: UnsafeCell<Option<T>>,
}


const IDLE: usize = 0;
const PUTTING: usize = 0b01;
const MARKING: usize = 0b10;
const HALTED: usize = 0b100;


pub(crate) enum PutResult<T>{
    TakenDuringPut(Option<T>),
    Success,
    TakenBeforePut(Option<T>),
    ConcurrentPut(Option<T>),
}
pub(crate) enum TakeResult<T>{
    Some(T),
    None,
    ConcurrentTake,
    ConcurrentPut,
}


impl<T> AtomicMarkerOption<T>{
    pub const fn new(value: Option<T>) -> Self {
        Self {
            mark: AtomicUsize::new(IDLE),
            value: UnsafeCell::new(value),
        }
    }
    pub fn put(&self,value: &T) where T: Clone{
        let mut arg = Err(value);
        loop{
            use PutResult::*;
            match self.try_put(Some(arg)) {
                Success => break,
                TakenDuringPut(Some(v)) | ConcurrentPut(Some(v)) | TakenBeforePut(Some(v)) => arg = Ok(v),
                TakenDuringPut(None) | ConcurrentPut(None) | TakenBeforePut(None) => arg = Err(value),
            }
            core::hint::spin_loop();
        }
    }
    pub fn clear(&self) where T: Clone{
        loop{
            use PutResult::*;
            match self.try_put(None) {
                Success => break,
                _ => {}
            }
            core::hint::spin_loop();
        }
    }
    pub fn try_put(&self,value: Option<Result<T,&T>>)->PutResult<T> where T: Clone{
        match self.mark.compare_exchange(IDLE, PUTTING, Acquire, Acquire).unwrap_or_else(identity) {
            IDLE => {
                unsafe {
                    // Locked acquired, update optional value
                    *self.value.get() = value.map(|v|v.unwrap_or_else(|v|v.clone()));
                    // Try release the lock
                    match self.mark.compare_exchange(PUTTING, IDLE, AcqRel, Acquire) {
                        Ok(_) => PutResult::Success,
                        Err(mark) => {
                            // This branch can only be reached if at least one
                            // concurrent thread called `take`.
                            debug_assert_eq!(mark, PUTTING | MARKING);
                            // Take already registered value.
                            let value = (*self.value.get()).take();
                            self.mark.swap(IDLE, AcqRel);
                            PutResult::TakenDuringPut(value)
                        }
                    }
                }
            }
            MARKING => PutResult::TakenBeforePut(value.and_then(|v|v.ok())),
            mark => {
                debug_assert!(mark == PUTTING || mark == PUTTING | MARKING);
                PutResult::ConcurrentPut(value.and_then(|v|v.ok()))
            }
        }
    }
    pub fn take(&self) -> TakeResult<T> {
        match self.mark.fetch_or(MARKING, AcqRel) {
            IDLE => { // Lock acquired
                let value = unsafe { (*self.value.get()).take() };
                // Release the lock
                self.mark.fetch_and(!MARKING, Release);
                match value {
                    Some(v) => TakeResult::Some(v),
                    None => TakeResult::None,
                }
            }
            mark => {
                // Already set MARKING bit
                debug_assert!(mark == PUTTING || mark == PUTTING | MARKING || mark == MARKING);
                if mark & MARKING != 0 { TakeResult::ConcurrentTake } else { TakeResult::ConcurrentPut }
            }
        }
    }
}

pub struct AtomicWaker {
    inner: AtomicMarkerOption<Waker>,
}
impl AtomicWaker{
    pub const fn empty() -> Self { Self { inner: AtomicMarkerOption::new(None) } }
    pub fn register(&self, waker: &Waker) {
        self.inner.put(waker)
    }


    pub fn clear(&self) {
        self.inner.clear();
    }
    pub fn notify_wake(&self) {

    }
}

pub struct WakerRegistry{
    waker: AtomicMarkerOption<Waker>,
}

impl WakerRegistry {
    pub const fn new() -> Self {
        Self {
            waker: AtomicMarkerOption::new(None)
        }
    }
    //register waker into slot, make sure it is settled and set flag indicating that all call to
    //notify_wake after this function must only set the atomic flag indicating that wake was present
    //and not call wake on waker registered by this fn.
    pub fn register(&self,waker: &Waker)->(bool,Result<(),Waker>){
       todo!()
    }
    fn take(&self) -> Option<Waker> {
        todo!()
    }

    //waker has been registered before this call, so all calls to notify_wake after this method will
    //wake it, if any call to notify_wake was present between register and start_listen, this method
    //will instead tak and wake waker by itself.
    pub fn start_listen(&self){

    }
    //clear waker previously registered by register method, and ignore mark flag if it was set after
    //call to register and before clear.
    pub fn clear(&self){
        self.waker.clear();
    }
    //saves state flag as notified, and in case registry started listening wakes waker.
    pub fn notify_waker(&self){

    }
}