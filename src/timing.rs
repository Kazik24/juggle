use std::time::{Instant, Duration};
use std::ops::{Add, Div};
use crate::chunk_slab::ChunkSlab;
use std::cmp::max;

pub(crate) trait Timing{
    type Duration: Copy + Ord + Add<Output=Self::Duration> + Default;
    type Instant;
    fn start(&self)->Self::Instant;
    fn stop(&self,start: Self::Instant)->Self::Duration;
    fn div_duration(dur: Self::Duration,by: u8)->Self::Duration;
    fn mul_percent(dur: Self::Duration,percent: u8)->Self::Duration;
}


pub(crate) struct TimingGroup<I: Timing>{
    info: ChunkSlab<usize,TimeEntry<I::Duration>>,
    max: I::Duration,
}
#[derive(Debug,Copy,Clone)]
struct TimeEntry<D>{
    sum: D,
    proportion: u8,
}

impl<I: Timing> TimingGroup<I>{

    pub fn new()->Self{
        Self{
            info: ChunkSlab::new(),
            max: I::Duration::default(),
        }
    }

    pub fn add(&mut self,proportion: u8)->usize{
        self.info.insert(TimeEntry{
            proportion,
            sum: I::Duration::default(),
        })
    }
    pub fn remove(&mut self,key: usize){ self.info.remove(key); }

    pub fn clear(&mut self){
        self.info.clear();
        self.max = I::Duration::default();
    }

    pub fn can_execute(&self,key: usize)->bool{
        let this = *self.info.get(key).expect("Error: unknown key passed to TimingGroup::can_execute");
        let this_dur = I::div_duration(this.sum,this.proportion);
        if this_dur == self.max {
            //check if there is any task with lower time used
            if self.info.iter().any(|(_,v)|v.sum != this_dur) {
                return false;
            }
        }
        let min_bound = I::mul_percent(self.max,90);
        let min_time = self.info.iter().map(|(k,v)|v.sum).min().unwrap();//there is at least one element in slab
        if min_time <= min_bound { //should execute tasks that are starved
            return this_dur <= min_bound; //execute ony if below bound
        }
        //execute normally
        true
    }
    pub fn update_duration(&mut self,key: usize,dur: I::Duration){
        let mut this = self.info.get_mut(key).expect("Error: unknown key passed to TimingGroup::update_duration");
        this.sum = this.sum + dur;

        let this_dur = I::div_duration(this.sum,this.proportion);
        self.max = max(self.max,this_dur);
    }

}
#[derive(Default)]
pub(crate) struct StdTiming;
impl Timing for StdTiming{
    type Duration = Duration;
    type Instant = Instant;
    #[inline]
    fn start(&self)->Self::Instant{ Instant::now() }
    #[inline]
    fn stop(&self,start: Self::Instant)->Self::Duration{ Instant::now() - start }
    #[inline]
    fn div_duration(dur: Self::Duration, by: u8) -> Self::Duration { dur.div(by as u32) }
    #[inline]
    fn mul_percent(dur: Self::Duration,percent: u8)->Self::Duration{ dur.mul_f32(percent as f32 / 100.0) }
}