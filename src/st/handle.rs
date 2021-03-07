use crate::st::algorithm::StaticAlgorithm;
use std::marker::PhantomData;
use crate::dy::IdNum;


pub struct StaticHandle{
    pub(crate) alg: &'static StaticAlgorithm,
    pub(crate) _phantom: PhantomData<*mut ()>,
    pub(crate) generation_id: usize,
}

impl StaticHandle{
    pub(crate) fn with_id(alg: &'static StaticAlgorithm, id: usize)->Self{
        Self{alg,_phantom:PhantomData,generation_id: id}
    }
    pub fn is_valid(&self)->bool{ self.alg.get_generation() == self.generation_id }

    pub fn cancel(&self, id: IdNum) -> bool {
        if !self.is_valid() {return false;}
        self.alg.cancel(id.to_usize())
    }
    pub fn suspend(&self, id: IdNum) -> bool {
        if !self.is_valid() {return false;}
        self.alg.suspend(id.to_usize())
    }
    pub fn resume(&self, id: IdNum) -> bool {
        if !self.is_valid() {return false;}
        self.alg.resume(id.to_usize())
    }
    pub fn restart(&self, id: IdNum) -> bool {
        if !self.is_valid() {return false;}
        self.alg.restart(id.to_usize())
    }
    pub fn current(&self) -> Option<IdNum> {
        if !self.is_valid() {return None;}
        self.alg.get_current().map(|t| IdNum::from_usize(t))
    }
    pub fn get_current_name(&self)->Option<&'static str> {
        if !self.is_valid() {return None;}
        self.alg.get_current().and_then(|k|self.alg.get_name(k))
    }
    pub fn get_name(&self, id: IdNum)->Option<&'static str> {
        if !self.is_valid() {return None;}
        self.alg.get_name(id.to_usize())
    }
    pub fn get_by_name(&self, name: &str)->Option<IdNum> {
        if !self.is_valid() {return None;}
        self.alg.get_by_name(name).map(|t|IdNum::from_usize(t))
    }
    pub fn registered_count(&self)-> usize{
        if !self.is_valid() {return 0;}
        self.alg.get_registered_count()
    }
    pub fn get_id_by_index(&self,index: usize)->IdNum{
        if !self.is_valid() { panic!("Handle is invalid.") }
        if index >= self.registered_count() {
            panic!("Error: Index out of bounds.");
        }
        IdNum::from_usize(index)
    }
}