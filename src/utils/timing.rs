use core::time::Duration;
use core::ops::{Add, Div, Mul, Sub};
use crate::chunk_slab::ChunkSlab;
use core::cmp::max;
use core::fmt::Debug;



pub trait TimerClock{
    type Duration: TimerCount;
    type Instant;
    fn start(&self)->Self::Instant;
    fn stop(&self,start: Self::Instant)->Self::Duration;
}

impl TimerCount for Duration{
    fn div_by(self, by: u16) -> Self { self.div(by as u32) }
    fn mul_by(self, by: u16) -> Self { self.mul(by as u32) }
}

macro_rules! impl_count {
    ($($name:ident),*) => {
        $(impl TimerCount for $name{
            fn div_by(self, by: u16) -> Self { self.div(by as Self) }
            fn mul_by(self, by: u16) -> Self { self.mul(by as Self) }
        })*
    }
}
// implement TimerCount for all integers except u8/i8
impl_count!(u16,i16,u32,i32,u64,i64,u128,i128,usize,isize);

pub trait TimerCount: Copy + Ord + Add<Output=Self> + Sub<Output=Self> + Default{
    fn div_by(self,by: u16)->Self;
    fn mul_by(self,by: u16)->Self;
}


pub struct TimingGroup<C: TimerCount>{
    info: ChunkSlab<usize,TimeEntry<C>>,
    max: C,
}
#[derive(Debug,Copy,Clone)]
struct TimeEntry<D>{
    sum: D,
    proportion: u16,
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

    /// Add entry to timing group with specific number of time slots and obtain its key.
    pub fn add(&mut self,proportion: u16)->usize{
        if proportion == 0 {panic!("Time slot count is zero.")}
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
    pub fn remove(&mut self,key: usize){ self.info.remove(key); }
    /// Returns time slot count for given key or None if this key is invalid.
    pub fn get_slot_count(&self,key: usize)->Option<u16>{ self.info.get(key).map(|v|v.proportion) }
    /// Remove all entries from this group and reset its state.
    pub fn clear(&mut self){
        self.info.clear();
        self.max = C::default();
    }

    /// Check if entry with specific key can be executed now.
    pub fn can_execute(&self,key: usize)->bool{
        let this = *self.info.get(key).expect("Error: unknown key passed to TimingGroup::can_execute");
        let this_dur = Self::get_proportional(&this);
        if this_dur == self.max {
            //check if there is any task with lower time used
            if self.info.iter().any(|(_,v)|Self::get_proportional(v) != this_dur) {
                return false;
            }
        }
        let min_bound = self.max.mul_by(9).div_by(10); // multiply by 0.9
        //there is at least one element in slab
        let min_time = self.info.iter().map(|(_,v)|Self::get_proportional(v)).min().unwrap();
        if min_time <= min_bound { //should execute tasks that are starved
            return this_dur <= min_bound; //execute ony if below bound
        }
        //execute normally
        true
    }

    /// Update execution duration of entry with specific key.
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

    fn get_proportional(entry: &TimeEntry<C>)->C{
        entry.sum.div_by(entry.proportion)
    }
}

/// Basic instance of TimerClock implemented using `std::time::Instant::now()`.
#[cfg(feature = "std")]
#[derive(Default,Clone,Eq,PartialEq,Hash,Debug)]
pub struct StdTimerClock;
#[cfg(feature = "std")]
impl TimerClock for StdTimerClock {
    type Duration = Duration;
    type Instant = std::time::Instant;
    #[inline]
    fn start(&self)->Self::Instant{ std::time::Instant::now() }
    #[inline]
    fn stop(&self,start: Self::Instant)->Self::Duration{ std::time::Instant::now() - start }
}