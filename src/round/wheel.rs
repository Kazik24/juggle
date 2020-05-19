use alloc::rc::Rc;
use core::cell::UnsafeCell;
use crate::round::algorithm::SchedulerAlgorithm;
use super::handle::*;
use core::future::Future;
use core::pin::Pin;
use core::task::*;



/// Single-thread async task scheduler with dynamic task spawning and cancelling.
///
/// This structure is useful when you want to divide single thread to share processing power among
/// multiple task without preemption.
/// Tasks are scheduled using round-robin algorithm without priority. Which means that all tasks
/// are not starved cause effectively they have all the same priority.
///
/// # Managing tasks
/// You can spawn/suspend/resume/cancel any task as long as you have it's key, and handle to this
/// scheduler. Handle can be obtained from this object using method `handle(&self)`
///
/// # Examples
/// ```
/// # extern crate alloc;
/// use juggle::*;
/// use alloc::collections::VecDeque;
/// use alloc::rc::Rc;
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
/// async fn collect_temperature(queue: Rc<RefCell<VecDeque<i32>>>,handle: WheelHandle){
///     loop{ // loop forever or until cancelled
///         let temperature: i32 = read_temperature_sensor().await;
///         queue.borrow_mut().push_back(temperature);
///         yield_once!(); // give scheduler opportunity to execute other tasks
///     }
/// }
///
/// async fn wait_for_timer(id: IdNum,queue: Rc<RefCell<VecDeque<i32>>>,handle: WheelHandle){
///     init_timer();
///     for _ in 0..5 {
///         yield_until!(get_timer_value() >= 200); // busy wait but also executes other tasks.
///         process_data(&mut queue.borrow_mut());
///         reset_timer();
///     }
///     handle.cancel(id); // cancel 'collect_temperature' task.
///     shutdown_timer();
/// }
///
/// fn main(){
///     let wheel = Wheel::new();
///     let handle = wheel.handle(); // handle to manage tasks, can be cloned inside this thread
///     let queue = Rc::new(RefCell::new(VecDeque::new()));
///
///     let temp_id = handle.spawn(SpawnParams::default(),
///                                collect_temperature(queue.clone(),handle.clone()));
///     handle.spawn(SpawnParams::default(),
///                  wait_for_timer(temp_id.unwrap(),queue.clone(),handle.clone()));
///
///     // execute tasks
///     smol::block_on(wheel); // or any other utility to block on future.
/// }
/// ```
pub struct Wheel{
    ptr: Rc<UnsafeCell<SchedulerAlgorithm>>,
    handle: WheelHandle,
}
/// Same as Wheel except that it has fixed content and there is no way to control state of tasks
/// within it.
/// [see] Wheel
pub struct LockedWheel{
    alg: SchedulerAlgorithm,
}

impl Wheel{
    pub fn new()->Self{
        let ptr = Rc::new(UnsafeCell::new(SchedulerAlgorithm::new()));
        let handle = WheelHandle::new(Rc::downgrade(&ptr));
        Self{ptr,handle}
    }

    pub fn handle(&self)->&WheelHandle{&self.handle}

    pub fn lock(self)->LockedWheel{
        // no panic cause rc has always strong count of 1 (it can have strong count > 1 during calls
        // on handle, but if these calls return then it will be back to 1)
        let alg = Rc::try_unwrap(self.ptr).ok().unwrap().into_inner();
        LockedWheel{alg}
    }
}

impl LockedWheel{
    pub fn unlock(self)->Wheel{
        let ptr = Rc::new(UnsafeCell::new(self.alg));
        let handle = WheelHandle::new(Rc::downgrade(&ptr));
        Wheel{ptr,handle}
    }
}


impl Future for Wheel{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe{&mut *self.as_mut().ptr.get() }.poll_internal(cx)
    }
}

impl Future for LockedWheel{
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_mut().alg.poll_internal(cx)
    }
}