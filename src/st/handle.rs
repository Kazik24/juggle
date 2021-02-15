use crate::st::algorithm::StaticAlgorithm;
use std::marker::PhantomData;
use crate::dy::IdNum;

#[derive(Copy,Clone)]
#[repr(transparent)]
pub struct StaticHandle{
    pub(crate) alg: &'static StaticAlgorithm,
    pub(crate) _phantom: PhantomData<*mut ()>,
}

impl StaticHandle{
    pub(crate) fn new(alg: &'static StaticAlgorithm)->Self{
        Self{alg,_phantom:PhantomData}
    }

    pub fn cancel(&self, id: IdNum) -> bool { self.alg.cancel(id.to_usize()) }
    pub fn suspend(&self, id: IdNum) -> bool { self.alg.suspend(id.to_usize()) }
    pub fn resume(&self, id: IdNum) -> bool { self.alg.resume(id.to_usize()) }
    pub fn restart(&self, id: IdNum) -> bool { self.alg.restart(id.to_usize()) }
    pub fn current(&self) -> Option<IdNum> { self.alg.get_current().map(|t| IdNum::from_usize(t)) }
    pub const fn registered_count(&self)-> usize{ self.alg.get_registered_count() }
    pub fn get_id_by_index(&self,index: usize)->IdNum{
        if index >= self.registered_count() {
            panic!("Error: Index out of bounds.");
        }
        IdNum::from_usize(index)
    }
}