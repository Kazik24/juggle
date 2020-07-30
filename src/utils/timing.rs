use core::time::Duration;
use core::ops::{Add, Div, Mul, Sub};
use crate::chunk_slab::ChunkSlab;
use core::cmp::max;
use core::fmt::Debug;



pub(crate) trait TimerClock{
    type Duration: TimerCount;
    type Instant;
    fn start(&self)->Self::Instant;
    fn stop(&self,start: Self::Instant)->Self::Duration;
}

impl TimerCount for Duration{
    fn div_duration(self, by: u16) -> Self { self.div(by as u32) }
    fn mul_duration(self, by: u16) -> Self { self.mul(by as u32) }
    fn mul_percent(self, percent: u16) -> Self { self.mul_f32(percent as f32 / 100.0) }
}

pub trait TimerCount: Copy + Ord + Add<Output=Self> + Sub<Output=Self> + Default{
    fn div_duration(self,by: u16)->Self;
    fn mul_duration(self,by: u16)->Self;
    fn mul_percent(self,percent: u16)->Self;
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

    pub fn new()->Self{
        Self{
            info: ChunkSlab::new(),
            max: C::default(),
        }
    }

    pub fn add(&mut self,proportion: u16)->usize{
        self.info.insert(TimeEntry{
            proportion,
            sum: C::default(),
            //above_threshold: false,
        })
    }
    pub fn remove(&mut self,key: usize){ self.info.remove(key); }
    #[allow(dead_code)]
    pub fn get_slot_count(&self,key: usize)->Option<u16>{ self.info.get(key).map(|v|v.proportion) }

    #[allow(dead_code)]
    pub fn clear(&mut self){
        self.info.clear();
        self.max = C::default();
    }

    pub fn can_execute(&self,key: usize)->bool{
        let this = *self.info.get(key).expect("Error: unknown key passed to TimingGroup::can_execute");
        let this_dur = Self::get_proportional(&this);
        if this_dur == self.max {
            //check if there is any task with lower time used
            if self.info.iter().any(|(_,v)|Self::get_proportional(v) != this_dur) {
                return false;
            }
        }
        let min_bound = self.max.mul_percent(90);
        //there is at least one element in slab
        let min_time = self.info.iter().map(|(_,v)|Self::get_proportional(v)).min().unwrap();
        if min_time <= min_bound { //should execute tasks that are starved
            return this_dur <= min_bound; //execute ony if below bound
        }
        //execute normally
        true
    }

    pub fn update_duration(&mut self,key: usize,dur: C){
        let this = self.info.get_mut(key).expect("Error: unknown key passed to TimingGroup::update_duration");
        this.sum = this.sum + dur;
        self.max = max(self.max,Self::get_proportional(this));
        let min_time = self.info.iter().map(|(_,v)|Self::get_proportional(v)).min().unwrap();
        if min_time != C::default() && false{
            self.max = self.max - min_time;
            for (_,entry) in self.info.iter_mut() { //offset all by disproportion
                entry.sum = entry.sum - min_time.mul_duration(entry.proportion);
            }
        }
    }

    fn get_proportional(entry: &TimeEntry<C>)->C{
        entry.sum.div_duration(entry.proportion)
    }
}

#[derive(Default)]
#[cfg(feature = "std")]
pub struct StdTiming;
#[cfg(feature = "std")]
impl TimerClock for StdTiming{
    type Duration = Duration;
    type Instant = std::time::Instant;
    #[inline]
    fn start(&self)->Self::Instant{ std::time::Instant::now() }
    #[inline]
    fn stop(&self,start: Self::Instant)->Self::Duration{ std::time::Instant::now() - start }
}