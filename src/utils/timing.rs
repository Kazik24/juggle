use core::time::Duration;
use core::ops::{Add, Div, Mul, Sub};
use crate::chunk_slab::ChunkSlab;
use core::cmp::max;
use core::fmt::{Debug, Formatter};
use core::num::NonZeroU16;


/// Custom time source. Implement this trait if you want to provide your own time source for
/// [LoadBalance](struct.LoadBalance.html) group.
///
/// # Examples
/// Example implementation using some arbitrary timer.
/// ```
/// use juggle::utils::TimerClock;
///
/// # fn init_timer()->u32{0}
/// # fn shutdown_timer(_:u32) {}
/// # fn get_timer_value(_:u32)->u32{0}
/// struct MyTimer{
///     timer_id: u32,
/// }
///
/// impl TimerClock for MyTimer {
///     type Duration = u32;
///     type Instant = u32;
///     fn start(&self) -> Self::Instant {
///         get_timer_value(self.timer_id)
///     }
///     fn stop(&self,start: Self::Instant) -> Self::Duration {
///         get_timer_value(self.timer_id) - start
///     }
/// }
///
/// impl MyTimer{
///     // make constructor that allocates and initializes hardware timer.
///     pub fn new()->Self{
///         Self{ timer_id: init_timer() }
///     }
/// }
/// impl Drop for MyTimer{
///     // release hardware resource
///     fn drop(&mut self){
///         shutdown_timer(self.timer_id);
///     }
/// }
/// ```
pub trait TimerClock{
    /// Type that represents time passed between two time points.
    type Duration: TimerCount;
    /// Custom type used as time point identifier, should represent value obtained from some
    /// monotonic clock.
    type Instant;
    /// Get current time value of this clock.
    fn start(&self)->Self::Instant;
    /// Get time passed between now and some past time point given in argument. `Instant` passed in
    /// argument is guaranteed to be obtained in some time point before this call, if clock is
    /// monotonic then condition `current_time_point >= start` can be relied on.
    fn stop(&self,start: Self::Instant)->Self::Duration;
}

/// Trait implemented by data types that can be used to measure time in abstract units.
pub trait TimerCount: Copy + Ord + Add<Output=Self> + Sub<Output=Self> + Default{
    /// Divide count by specified value that is not zero.
    fn div_by(self,by: NonZeroU16)->Self;
    /// Multiply count by specified value that is not zero.
    fn mul_by(self,by: NonZeroU16)->Self;
}

macro_rules! impl_count {
    ($($name:ident),*) => {
        $(impl TimerCount for $name{
            fn div_by(self, by: NonZeroU16) -> Self { self.div(by.get() as Self) }
            fn mul_by(self, by: NonZeroU16) -> Self { self.mul(by.get() as Self) }
        })*
    }
}
// implement TimerCount for all integers except u8/i8
impl_count!(u16,i16,u32,i32,u64,i64,u128,i128,usize,isize);

impl TimerCount for Duration{
    fn div_by(self, by: NonZeroU16) -> Self { self.div(by.get() as u32) }
    fn mul_by(self, by: NonZeroU16) -> Self { self.mul(by.get() as u32) }
}

/// Helper for equally dividing time slots across multiple entries manually.
///
/// This struct can be used to control time usage of some arbitrary entries and to ensure they have
/// more-less equal amount of time per slot to assign. [LoadBalance](struct.LoadBalance.html)
/// group uses this struct to split time slots between tasks.
///
/// You can use custom time units by implementing trait [TimerCount](trait.TimerCount.html).
pub struct TimingGroup<C: TimerCount>{
    info: ChunkSlab<usize,TimeEntry<C>>,
    max: C,
}
#[derive(Debug,Copy,Clone)]
struct TimeEntry<D>{
    sum: D,
    proportion: NonZeroU16,
    //above_threshold: bool,
}

impl<C: TimerCount> TimingGroup<C>{

    /// Create new empty instance of `TimingGroup`.
    pub fn new()->Self{
        Self{
            info: ChunkSlab::new(),
            max: C::default(),
        }
    }
    /// Create new empty instance of `TimingGroup` that can contain at least `capacity` number of
    /// entries without reallocating.
    pub fn with_capacity(capacity: usize)->Self{
        Self{
            info: ChunkSlab::with_capacity(capacity),
            max: C::default(),
        }
    }
    /// Insert entry to timing group with specific number of time slots and obtain its key.
    ///
    /// # Panics
    /// Panics if time slot count argument is zero.
    pub fn insert(&mut self, slot_count: u16) ->usize{
        let proportion = NonZeroU16::new(slot_count).expect("Time slot count is zero.");
        self.info.insert(TimeEntry{
            proportion,
            sum: C::default(),
            //above_threshold: false,
        })
    }
    /// Remove entry with specific key from this timing group.
    ///
    /// # Panics
    /// Panics if provided key has no associated entry.
    pub fn remove(&mut self,key: usize){
        self.info.remove(key).expect("Error: unknown key passed to TimingGroup::remove");
    }
    /// Checks if specific key has an entry associated to it in this group.
    pub fn contains(&self,key: usize)->bool{
        self.info.get(key).is_some()
    }
    /// Returns number of registered entries in this group.
    pub fn count(&self)->usize{ self.info.len() }
    /// Returns time slot count for given key or None if this key is invalid.
    pub fn get_slot_count(&self,key: usize)->Option<u16>{ self.info.get(key).map(|v|v.proportion.get()) }
    /// Remove all entries from this group and reset its state.
    pub fn clear(&mut self){
        self.info.clear();
        self.max = C::default();
    }

    /// Check if entry with specific key can be executed now.
    ///
    /// # Panics
    /// Panics if provided key has no associated entry.
    pub fn can_execute(&self,key: usize)->bool{
        let this = *self.info.get(key).expect("Error: unknown key passed to TimingGroup::can_execute");
        let this_dur = Self::get_proportional(&this);
        if this_dur == self.max {
            //check if there is any task with lower time used
            if self.info.iter().any(|(_,v)|Self::get_proportional(v) != this_dur) {
                return false;
            }
        }
        // multiply max by 0.9
        let min_bound = self.max.mul_by(NonZeroU16::new(9).unwrap()).div_by(NonZeroU16::new(10).unwrap());
        //there is at least one element in slab
        let min_time = self.info.iter().map(|(_,v)|Self::get_proportional(v)).min().unwrap();
        if min_time <= min_bound { //should execute tasks that are starved
            return this_dur <= min_bound; //execute ony if below bound
        }
        //execute normally
        true
    }

    /// Update execution duration of entry with specific key.
    ///
    /// # Panics
    /// Panics if provided key has no associated entry.
    pub fn update_duration(&mut self,key: usize,dur: C){
        let this = self.info.get_mut(key).expect("Error: unknown key passed to TimingGroup::update_duration");
        this.sum = this.sum + dur;
        self.max = max(self.max,Self::get_proportional(this));
        let min_time = self.info.iter().map(|(_,v)|Self::get_proportional(v)).min().unwrap();
        if min_time != C::default() && false{
            self.max = self.max - min_time;
            for (_,entry) in self.info.iter_mut() { //offset all by disproportion
                entry.sum = entry.sum - min_time.mul_by(entry.proportion);
            }
        }
    }
    #[inline]
    fn get_proportional(entry: &TimeEntry<C>)->C{
        entry.sum.div_by(entry.proportion)
    }
}

impl<C: TimerCount + Debug> Debug for TimingGroup<C>{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        struct DebugEntry<'a,T: Debug>(&'a TimeEntry<T>);
        impl<'a,T:Debug> Debug for DebugEntry<'a,T>{
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                f.debug_struct("")
                    .field("slots",&self.0.proportion.get())
                    .field("total",&self.0.sum).finish()
            }
        }
        let mut m = f.debug_map();
        for (key,entry) in self.info.iter(){
            m.entry(&key,&DebugEntry(entry));
        }
        m.finish()
    }
}

/// Basic instance of TimerClock implemented using `std::time::Instant::now()`.
///
/// Feature `std` is required to use this struct.
#[cfg(feature = "std")]
#[derive(Default,Clone,Eq,PartialEq,Hash,Debug)]
pub struct StdTimerClock;
#[cfg(feature = "std")]
impl TimerClock for StdTimerClock {
    type Duration = Duration;
    type Instant = std::time::Instant;
    #[inline]
    fn start(&self)->Self::Instant{ Self::Instant::now() }
    #[inline]
    fn stop(&self,start: Self::Instant)->Self::Duration{ Self::Instant::now() - start }
}