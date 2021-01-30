use alloc::rc::Rc;
use core::fmt::{Display, Formatter};
use core::fmt::Debug;
use core::future::Future;
use core::pin::Pin;
use core::task::*;
use super::handle::*;
use crate::round::Algorithm;

/// Single-thread async task scheduler with dynamic task state control. Implements `Future`.
///
/// This structure is useful when you want to divide single thread to share processing power among
/// multiple task by [cooperative multitasking](https://en.wikipedia.org/wiki/Cooperative_multitasking).
///
/// Tasks are scheduled using round-robin algorithm without priority. Diagram below shows simplified
/// scheduling process.
/// ```text
///    Execute
///       |
///    +--v--+     +-----+-----+-----+-----+
///    |     |     |     |     |     |     <---+
///    | 0x0 | <-- | 0x8 | 0x9 | 0xA | 0xB |   |
///    |     |     |     |     |     |     <-+ |
///    +-----+ ... +-----+-----+-----+-----+ | |
///       |                                  | |
///       |                                  | |
///   +---v---+                              | |
///  /         \ YES                         | |
/// + Runnable? +----------------------------+ |
///  \         /                               |
///   +-------+                                |
///       | NO      +-----------------------+  |
///       +---------> Wait for waking event |--+
///                 +-----------------------+
/// ```
/// New and rescheduled tasks are added at the end of queue. Task on front is poped and executed.
/// If task finishes or becomes cancelled then it is removed from scheduler, and if task is
/// suspended then it is also removed but only from queue not the scheduler.
/// Tasks that were blocked by some async event are checked periodically and when woken they are
/// added at the end of queue, this might not happen immedialty after task is woken.
///
/// # Managing tasks
/// You can [spawn]/[suspend]/[resume]/[cancel] any task as long as you have its [identifier], and
/// [handle](struct.WheelHandle.html) to this scheduler. Handle can be obtained from this
/// object using [that method](#method.handle), and task identifiers are returned from
/// functions that spawn tasks (e.g [`spawn`](struct.WheelHandle.html#method.spawn)).
///
/// Be carefull tho with suspending tasks, if all become suspended then because of scheduler
/// single-thread nature there is no way of resuming any. Such condition triggers
/// [`SuspendError`](struct.SuspendError.html) that is returned from future.
///
/// [spawn]: struct.WheelHandle.html#method.spawn
/// [cancel]: struct.WheelHandle.html#method.cancel
/// [suspend]: struct.WheelHandle.html#method.suspend
/// [resume]: struct.WheelHandle.html#method.resume
/// [identifier]: struct.IdNum.html
/// # Examples
/// ```
/// # extern crate alloc;
/// use juggle::*;
/// use alloc::collections::VecDeque;
/// use core::cell::RefCell;
///
/// # use core::sync::atomic::*;
/// # async fn read_temperature_sensor()->i32 { 10 }
/// # fn init_timer(){}
/// # static CNT: AtomicUsize = AtomicUsize::new(0);
/// # fn get_timer_value()->u32 { CNT.fetch_add(1,Ordering::Relaxed) as _ }
/// # fn reset_timer(){CNT.store(0,Ordering::Relaxed);}
/// # fn shutdown_timer(){}
/// # fn process_data(queue: &mut VecDeque<i32>){
/// #     while let Some(_) = queue.pop_front() { }
/// # }
/// async fn collect_temperature(queue: &RefCell<VecDeque<i32>>,handle: WheelHandle<'_>){
///     loop{ // loop forever or until cancelled
///         let temperature: i32 = read_temperature_sensor().await;
///         queue.borrow_mut().push_back(temperature);
///         yield_once!(); // give scheduler opportunity to execute other tasks
///     }
/// }
///
/// async fn wait_for_timer(id: IdNum,queue: &RefCell<VecDeque<i32>>,handle: WheelHandle<'_>){
///     init_timer();
///     for _ in 0..5 {
///         yield_while!(get_timer_value() < 200); // busy wait but also executes other tasks.
///         process_data(&mut queue.borrow_mut());
///         reset_timer();
///     }
///     handle.cancel(id); // cancel 'collect_temperature' task.
///     shutdown_timer();
/// }
///
/// fn main(){
///     let queue = &RefCell::new(VecDeque::new());
///     let wheel = Wheel::new();
///     let handle = wheel.handle(); // handle to manage tasks, can be cloned inside this thread
///
///     let temp_id = handle.spawn(SpawnParams::default(),
///                                collect_temperature(queue,handle.clone()));
///     handle.spawn(SpawnParams::default(),
///                  wait_for_timer(temp_id.unwrap(),queue,handle.clone()));
///
///     // execute tasks
///     smol::block_on(wheel).unwrap(); // or any other utility to block on future.
/// }
/// ```
pub struct Wheel<'futures> {
    ptr: Rc<Algorithm<'futures>>,
    handle: WheelHandle<'futures>,
}

/// Same as [`Wheel`](struct.Wheel.html) except that it has fixed content and there is no way to
/// control state of tasks within it. Implements `Future`.
pub struct LockedWheel<'futures> {
    alg: Algorithm<'futures>,
}

impl<'futures> Wheel<'futures> {
    /// Create new instance
    pub fn new() -> Self {
        Self::from_inner(Algorithm::new())
    }

    fn from_inner(alg: Algorithm<'futures>) -> Self {
        let ptr = Rc::new(alg);
        let handle = WheelHandle::new(Rc::downgrade(&ptr));
        Self { ptr, handle }
    }

    /// Obtain reference to handle that is used to spawn/control tasks.
    ///
    /// Any interaction with `Wheel`'s content is done by handles. Handles works as reference
    /// counted pointers, they can be cloned to use inside tasks but cannot be shared
    /// between threads.
    pub fn handle(&self) -> &WheelHandle<'futures> { &self.handle }

    /// Lock this wheel preventing all handles from affecting the tasks.
    ///
    /// Transforms this instance into [`LockedWheel`](struct.LockedWheel.html) which is similar to
    /// [`Wheel`](struct.Wheel.html) but has no way of controlling tasks within it.
    ///
    /// # Panics
    /// Panics if this method was called inside handle's method such as
    /// [`with_name`](struct.WheelHandle.html#method.with_name).
    pub fn lock(self) -> LockedWheel<'futures> {
        // rc has always strong count of 1 (it can have strong count > 1 during calls
        // on handle, but if these calls return then it will be back to 1)
        let alg = Rc::try_unwrap(self.ptr).ok().expect("Cannot lock inside call to handle's method.");
        LockedWheel { alg }
    }
}

impl<'futures> LockedWheel<'futures> {
    /// Unlock this wheel so that a handle can be obtained and used to spawn or control tasks.
    ///
    /// Transforms this instance back to [`Wheel`](struct.Wheel.html)
    /// Note that handles which were invalidated after this wheel was locked won't be valid
    /// again after calling this method, new [`handle`](struct.Wheel.html#method.handle) should be obtained.
    pub fn unlock(self) -> Wheel<'futures> {
        Wheel::from_inner(self.alg)
    }
}

impl<'futures> Debug for Wheel<'futures> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.ptr.format_internal(f, "Wheel")
    }
}

impl<'futures> Debug for LockedWheel<'futures> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.alg.format_internal(f, "LockedWheel")
    }
}

impl<'futures> Default for Wheel<'futures> {
    fn default() -> Self { Self::new() }
}


impl<'futures> Future for Wheel<'futures> {
    type Output = Result<(), SuspendError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_ref().ptr.poll_internal(cx).map(|flag| if flag { Ok(()) } else { Err(SuspendError) })
    }
}

impl<'futures> Future for LockedWheel<'futures> {
    type Output = Result<(), SuspendError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_ref().alg.poll_internal(cx).map(|flag| if flag { Ok(()) } else { Err(SuspendError) })
    }
}


/// Error returned by scheduler's `Future` when all tasks become suspended.
///
/// [`Wheel`](struct.Wheel.html)/[`LockedWheel`](struct.LockedWheel.html) can only operate within single
/// thread so if all tasks in it become suspended, then it cannot continue execution because there
/// is no way to resume any task. When such situation occurs, this error is returned by scheduler
/// `Future`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct SuspendError;

impl Display for SuspendError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("All tasks were suspended.")
    }
}