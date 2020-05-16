use core::mem::replace;
use alloc::boxed::Box;
use alloc::vec::Vec;

const CHUNK_SIZE: usize = 16;


pub(crate) struct ChunkSlab<I: ChunkSlabKey,T>{
    entries: Vec<Box<Chunk<I,T>>>,
    len: usize,
    next: I,
}

struct Chunk<I: ChunkSlabKey,T>{
    data: [Entry<I,T>;CHUNK_SIZE],
}
impl<I: ChunkSlabKey,T> Chunk<I,T>{
    pub fn init(mut start: I,first: T)->Self{
        let mut data: [Entry<I,T>;CHUNK_SIZE] = Default::default();
        data[0] = Entry::Full(first);
        start = start.add_one();
        for elem in data[1..].iter_mut(){
            start = I::from_index(start.into_index().wrapping_add(1));
            *elem = Entry::Empty(start);
        }
        Self{data}
    }
}


#[repr(u8)] enum Entry<I: ChunkSlabKey,T>{
    Empty(I),
    Full(T),
}
impl<I: ChunkSlabKey,T> Default for Entry<I,T>{
    fn default() -> Self {Self::Empty(I::zero())}
}

impl<I: ChunkSlabKey,T> ChunkSlab<I,T>{
    pub fn new()->Self{
        Self{
            entries: Vec::new(),
            next: I::zero(),
            len: 0,
        }
    }
    pub fn insert(&mut self,val: T)->I{
        if I::max_value() != usize::max_value(){
            if self.len.saturating_sub(1) == I::max_value(){
                panic!("ChunkSlab size overflow.");
            }
        }

        let key = self.next;
        self.len += 1;
        if key.into_index() == self.entries.len()*CHUNK_SIZE {
            self.entries.push(Box::new(Chunk::init(key,val)));
            self.next = key.add_one();
        } else {
            let chunk = &mut self.entries[key.into_index() / CHUNK_SIZE];
            let prev = replace(&mut chunk.data[key.into_index() % CHUNK_SIZE], Entry::Full(val));

            match prev {
                Entry::Empty(next) => {
                    self.next = next;
                }
                _ => unreachable!(),
            }
        }
        key
    }

    pub fn remove(&mut self, key: I) -> T {
        let chunk = self.entries.get_mut(key.into_index() / CHUNK_SIZE).expect("Invalid key");
        let sub_idx = key.into_index() % CHUNK_SIZE;
        let prev = replace(&mut chunk.data[sub_idx], Entry::Empty(self.next));

        match prev {
            Entry::Full(val) => {
                self.len -= 1;
                self.next = key;
                val
            }
            _ => {
                // entry is empty, restore state
                chunk.data[sub_idx] = prev;
                panic!("Invalid key");
            }
        }
    }
    pub fn clear(&mut self){
        self.entries.clear();
        self.len = 0;
        self.next = I::zero();
    }
    pub fn len(&self)->usize{self.len}
    pub fn contains(&self,key: I)->bool{ self.get(key).is_some() }
    pub fn get(&self,key: I)->Option<&T>{
        match self.entries.get(key.into_index() / CHUNK_SIZE) {
            Some(chunk) =>{
                match &chunk.data[key.into_index() % CHUNK_SIZE] {
                    Entry::Full(data) => Some(data),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    pub fn get_mut(&mut self,key: I)->Option<&mut T> {
        match self.entries.get_mut(key.into_index() / CHUNK_SIZE) {
            Some(chunk) =>{
                match &mut chunk.data[key.into_index() % CHUNK_SIZE] {
                    Entry::Full(data) => Some(data),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

pub(crate) trait ChunkSlabKey: Copy{
    fn into_index(self)->usize;
    fn from_index(idx: usize)->Self;
    fn max_value()->usize;
    fn zero()->Self;
    fn add_one(self)->Self;
}
macro_rules! impl_key{
    ($($typename:ty),*) => {
        $(impl ChunkSlabKey for $typename {
            fn into_index(self) -> usize { self as usize }
            fn from_index(idx: usize) -> Self { idx as Self }
            fn max_value()->usize{ Self::max_value() as usize }
            fn zero()->Self{ 0 }
            fn add_one(self)->Self{ self + 1 }
        })*
    }
}

impl_key!(u8,u16,u32,usize);



#[cfg(test)]
mod tests{
    use super::*;
    use core::fmt::*;


    fn under_test<I: ChunkSlabKey>(max_key: I)->ChunkSlab<I,i32>{
        let mut slab = ChunkSlab::new();
        for i in 0..=max_key.into_index() {
            slab.insert(i.wrapping_add(10) as i32);
        }
        slab
    }
    #[allow(dead_code)]
    fn print_view<I: ChunkSlabKey + Debug,T: Debug>(slab: &ChunkSlab<I,T>)->String{
        slab.entries.iter().map(|e|{
            format!("Chunk[{}]",e.data.iter().map(|e|{
                match e {
                    Entry::Full(v) => format!("Full({:?})",v),
                    Entry::Empty(v) => format!("Empt({:?})",v),
                }
            }).collect::<Vec<_>>().join(", "))
        }).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn test_fill(){
        let mut slab = under_test(255u8);
        for i in 0..=255{
            assert_eq!(slab.get(i), Some(&(i as i32 + 10)));
            assert_eq!(slab.get_mut(i), Some(&mut (i as i32 + 10)));
        }
    }
    #[test]
    #[should_panic]
    fn test_overflow(){
        let mut slab = ChunkSlab::<u8,_>::new();
        for _ in 0..=(u8::max_value() as usize+1) {
            slab.insert(1);
        }
    }
    #[test]
    #[should_panic]
    fn test_overflow_big(){
        let mut slab = ChunkSlab::<u16,_>::new();
        for _ in 0..=(u16::max_value() as usize+1) {
            slab.insert(1);
        }
    }
    #[test]
    fn test_part_fill(){
        for val in vec![2u8,7,15,16,17,31,32,33,100,200] {
            let mut slab = under_test(val);
            for i in 0..=255{
                if i <= val {
                    assert_eq!(slab.get(i), Some(&(i as i32 + 10)));
                    assert_eq!(slab.get_mut(i), Some(&mut (i as i32 + 10)));
                }else{
                    assert_eq!(slab.get(i), None);
                    assert_eq!(slab.get_mut(i), None);
                }
            }
        }
    }
    #[test]
    fn test_remove(){
        let mut slab = ChunkSlab::<u8,_>::new();
        let k1 = slab.insert(10);
        let k2 = slab.insert(20);
        let k3 = slab.insert(30);
        assert_eq!(slab.len(),3);
        assert_eq!(slab.remove(k2),20);
        assert_eq!(slab.remove(k1),10);
        assert_eq!(slab.len(),1);
        let k4 = slab.insert(40);
        let k5 = slab.insert(50);
        assert_eq!(slab.get(k4),Some(&40));
        assert_eq!(slab.get(k5),Some(&50));
        slab.clear();
        assert_eq!(slab.len(),0);
        println!("Keys: {},{},{},{},{}",k1,k2,k3,k4,k5);
        assert_eq!(slab.get(k1),None);
        assert_eq!(slab.get(k2),None);
        assert_eq!(slab.get(k3),None);
        assert_eq!(slab.get(k4),None);
        assert_eq!(slab.get(k5),None);
    }
    #[test]
    fn test_pinning(){ //never realloc structures, required by scheduler algorithm
        let mut slab = ChunkSlab::<u8,_>::new();
        let k1 = slab.insert(10);
        let k2 = slab.insert(20);
        let ptr1 = slab.get(k1).unwrap() as *const _;
        let ptr2 = slab.get(k2).unwrap() as *const _;
        for _ in 0..200 {
            slab.insert(30);
        }
        let ptr11 = slab.get(k1).unwrap() as *const _;
        let ptr22 = slab.get(k2).unwrap() as *const _;
        assert!(core::ptr::eq(ptr1,ptr11));
        assert!(core::ptr::eq(ptr2,ptr22));
    }
}
