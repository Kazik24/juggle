use std::time::{Instant, Duration};
use std::ops::{Add, Div};
use crate::chunk_slab::ChunkSlab;
use std::cmp::max;

pub(crate) trait Timing{
    type TimePoint: TimePoint;
    fn start()->Self::TimePoint;
}

pub(crate) trait TimePoint{
    type Duration: Copy + Ord + Add<Output=Self::Duration> + Div<u32,Output=Self::Duration>;
    fn duration_from(self)->Self::Duration;
    fn zero_duration()->Self::Duration;
    fn mul_percent(dur: Self::Duration,percent: u8)->Self::Duration;
}

pub(crate) struct TimingGroup<I: Timing>{
    info: ChunkSlab<usize,TimeEntry<<I::TimePoint as TimePoint>::Duration>>,
    max: <I::TimePoint as TimePoint>::Duration,
}
#[derive(Debug,Copy,Clone)]
struct TimeEntry<D>{
    sum: D,
    proportion: u32,
}

impl<I: Timing> TimingGroup<I>{

    pub fn new()->Self{
        Self{
            info: ChunkSlab::new(),
            max: I::TimePoint::zero_duration(),
        }
    }

    pub fn add(&mut self,proportion: u32)->usize{
        self.info.insert(TimeEntry{
            proportion,
            sum: I::TimePoint::zero_duration(),
        })
    }
    pub fn remove(&mut self,key: usize){ self.info.remove(key); }

    pub fn clear(&mut self){
        self.info.clear();
        self.max = I::TimePoint::zero_duration();
    }

    pub fn can_execute(&self,key: usize)->bool{
        let this = *self.info.get(key).expect("Error: unknown key passed to TimingGroup::can_execute");
        let this_dur = this.sum / this.proportion;
        if this_dur == self.max {
            //check if there is any task with lower time used
            if self.info.iter().any(|(_,v)|v.sum != this_dur) {
                return false;
            }
        }
        let min_bound = I::TimePoint::mul_percent(self.max,90);
        let min_time = self.info.iter().map(|(k,v)|v.sum).min().unwrap();//there is at least one element in slab
        if min_time <= min_bound { //should execute tasks that are starved
            return this_dur <= min_bound; //execute ony if below bound
        }
        //execute normally
        true
    }
    pub fn update_duration(&mut self,key: usize,dur: <I::TimePoint as TimePoint>::Duration){
        let mut this = self.info.get_mut(key).expect("Error: unknown key passed to TimingGroup::update_duration");
        this.sum = this.sum + dur;

        let this_dur = this.sum / this.proportion;
        self.max = max(self.max,this_dur);
    }

}




pub(crate) struct StdTiming;
pub(crate) struct StdTimePoint(Instant);
impl Timing for StdTiming{
    type TimePoint = StdTimePoint;
    fn start() -> Self::TimePoint {StdTimePoint(Instant::now())}
}
impl TimePoint for StdTimePoint{
    type Duration = Duration;
    fn duration_from(self) -> Self::Duration {Instant::now() - self.0}
    fn zero_duration()-> Self::Duration { Duration::from_millis(0) }
    fn mul_percent(dur: Self::Duration, percent: u8) -> Self::Duration {
        dur.mul_f32(percent as f32 / 100.0)
    }
}