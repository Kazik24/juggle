use alloc::boxed::Box;
use core::mem::replace;
use smallvec::SmallVec;

const CHUNK_SIZE: usize = 16;


pub(crate) struct ChunkSlab<I: ChunkSlabKey, T> {
    entries: SmallVec<[Box<Chunk<I, T>>;2]>, //store 2*CHUNK_SIZE tasks with one less indirection
    len: usize,
    next: I,
}

struct Chunk<I: ChunkSlabKey, T> {
    data: [Entry<I, T>; CHUNK_SIZE],
}

impl<I: ChunkSlabKey, T> Chunk<I, T> {
    pub fn init(mut start: I, first: T) -> Self {
        let mut data: [Entry<I, T>; CHUNK_SIZE] = Default::default();
        data[0] = Entry::Full(first);
        start = start.add_one();
        for elem in &mut data[1..] {
            start = I::from_index(start.into_index().wrapping_add(1));
            *elem = Entry::Empty(start);
        }
        Self { data }
    }
}


#[repr(u8)]
enum Entry<I: ChunkSlabKey, T> {
    Empty(I),
    Full(T),
}

impl<I: ChunkSlabKey, T> Default for Entry<I, T> {
    fn default() -> Self { Self::Empty(I::zero()) }
}

impl<I: ChunkSlabKey, T> ChunkSlab<I, T> {
    pub fn new() -> Self {
        Self {
            entries: SmallVec::new(),
            next: I::zero(),
            len: 0,
        }
    }
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: SmallVec::with_capacity(cap / CHUNK_SIZE + if cap % CHUNK_SIZE != 0 { 1 } else { 0 }),
            next: I::zero(),
            len: 0,
        }
    }
    pub fn insert(&mut self, val: T) -> I {
        if I::max_value() != usize::max_value() {
            if self.len.saturating_sub(1) == I::max_value() {
                panic!("ChunkSlab size overflow.");
            }
        }

        let key = self.next;
        self.len += 1;
        if key.into_index() == self.entries.len() * CHUNK_SIZE {
            self.entries.push(Box::new(Chunk::init(key, val)));
            self.next = key.add_one();
        } else {
            let chunk = &mut self.entries[key.into_index() / CHUNK_SIZE];
            let prev = replace(&mut chunk.data[key.into_index() % CHUNK_SIZE], Entry::Full(val));

            match prev {
                Entry::Empty(next) => self.next = next,
                _ => unreachable!(),
            }
        }
        key
    }

    pub fn remove(&mut self, key: I) -> Option<T> {
        let chunk = self.entries.get_mut(key.into_index() / CHUNK_SIZE)?;
        let sub_idx = key.into_index() % CHUNK_SIZE; //cannot cause IOOB when accessing chunk.data
        let elem = &mut chunk.data[sub_idx];
        match replace(elem, Entry::Empty(self.next)) {
            Entry::Full(val) => {
                self.len -= 1;
                self.next = key;
                Some(val)
            }
            prev => {
                // entry is empty, restore state
                *elem = prev;
                None
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.len = 0;
        self.next = I::zero();
    }

    pub fn len(&self) -> usize { self.len }
    pub fn get(&self, key: I) -> Option<&T> {
        match self.entries.get(key.into_index() / CHUNK_SIZE) {
            Some(chunk) => {
                match &chunk.data[key.into_index() % CHUNK_SIZE] {
                    Entry::Full(data) => Some(data),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: I) -> Option<&mut T> {
        match self.entries.get_mut(key.into_index() / CHUNK_SIZE) {
            Some(chunk) => {
                match &mut chunk.data[key.into_index() % CHUNK_SIZE] {
                    Entry::Full(data) => Some(data),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    pub fn iter(&self) -> impl Iterator<Item=(I, &T)> {
        self.entries.iter().enumerate().flat_map(|e| {
            let off = e.0 * CHUNK_SIZE;
            e.1.data.iter().enumerate().map(move |e| (off + e.0, e.1))
        }).filter_map(|e| {
            match e.1 {
                Entry::Full(v) => Some((I::from_index(e.0), v)),
                Entry::Empty(_) => None,
            }
        })
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item=(I, &mut T)> {
        self.entries.iter_mut().enumerate().flat_map(|e| {
            let off = e.0 * CHUNK_SIZE;
            e.1.data.iter_mut().enumerate().map(move |e| (off + e.0, e.1))
        }).filter_map(|e| {
            match e.1 {
                Entry::Full(v) => Some((I::from_index(e.0), v)),
                Entry::Empty(_) => None,
            }
        })
    }

    pub fn retain(&mut self,mut func: impl FnMut(I,&T)->bool){
        for (ci,chunk) in self.entries.iter_mut().enumerate() {
            let offset = ci*CHUNK_SIZE;
            for (i,val) in chunk.data.iter_mut().enumerate() {
                if let Entry::Full(value) = val {
                    let key = I::from_index(offset + i);
                    if !func(key,value) { //remove
                        *val = Entry::Empty(self.next);
                        self.len -= 1;
                        self.next = key;
                    }
                }
            }
        }
    }
}

pub(crate) trait ChunkSlabKey: Copy {
    fn into_index(self) -> usize;
    fn from_index(idx: usize) -> Self;
    fn max_value() -> usize;
    fn zero() -> Self;
    fn add_one(self) -> Self;
}
macro_rules! impl_key {
    ($($typename:ty),*) => {
        $(impl ChunkSlabKey for $typename {
            fn into_index(self) -> usize { self as usize }
            fn from_index(idx: usize) -> Self { idx as Self }
            fn max_value()->usize{ Self::MAX as usize }
            fn zero()->Self{ 0 }
            fn add_one(self)->Self{ self + 1 }
        })*
    }
}

impl_key!(u8,u16,u32,usize);



#[cfg(test)]
mod tests {
    extern crate std;
    use alloc::string::String;
    use alloc::collections::BTreeSet;
    use alloc::vec::Vec;
    use core::fmt::*;
    use core::iter::successors;
    use super::*;
    use rand::prelude::StdRng;
    use rand::{SeedableRng, Rng};

    fn under_test<I: ChunkSlabKey>(max_key: I) -> ChunkSlab<I, i32> {
        let mut slab = ChunkSlab::new();
        for i in 0..=max_key.into_index() {
            slab.insert(i.wrapping_add(10) as i32);
        }
        slab
    }

    #[allow(dead_code)]
    fn print_view<I: ChunkSlabKey + Debug, T: Debug>(slab: &ChunkSlab<I, T>) -> String {
        slab.entries.iter().map(|e| {
            std::format!("Chunk[{}]",e.data.iter().map(|e|{
                match e {
                    Entry::Full(v) => alloc::format!("Full({:?})",v),
                    Entry::Empty(v) => alloc::format!("Empt({:?})",v),
                }
            }).collect::<Vec<_>>().join(", "))
        }).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn test_fill() {
        let mut slab = under_test(255u8);
        for i in 0..=255 {
            assert_eq!(slab.get(i), Some(&(i as i32 + 10)));
            assert_eq!(slab.get_mut(i), Some(&mut (i as i32 + 10)));
        }
    }

    #[test]
    #[should_panic]
    fn test_overflow() {
        let mut slab = ChunkSlab::<u8, _>::new();
        for _ in 0..=(u8::max_value() as usize + 1) {
            slab.insert(1);
        }
    }

    #[test]
    #[should_panic]
    fn test_overflow_big() {
        let mut slab = ChunkSlab::<u16, _>::new();
        for _ in 0..=(u16::max_value() as usize + 1) {
            slab.insert(1);
        }
    }

    #[test]
    fn test_part_fill() {
        for val in [2u8, 7, 15, 16, 17, 31, 32, 33, 100, 200].iter().copied() {
            let mut slab = under_test(val);
            for i in 0..=255 {
                if i <= val {
                    assert_eq!(slab.get(i), Some(&(i as i32 + 10)));
                    assert_eq!(slab.get_mut(i), Some(&mut (i as i32 + 10)));
                } else {
                    assert_eq!(slab.get(i), None);
                    assert_eq!(slab.get_mut(i), None);
                }
            }
        }
    }

    #[test]
    fn test_remove() {
        let mut slab = ChunkSlab::<u8, _>::new();
        let k1 = slab.insert(10);
        let k2 = slab.insert(20);
        let k3 = slab.insert(30);
        assert_eq!(slab.len(), 3);
        assert_eq!(slab.remove(k2), Some(20));
        assert_eq!(slab.remove(k2), None);
        assert_eq!(slab.remove(k1), Some(10));
        assert_eq!(slab.remove(k1), None);
        assert_eq!(slab.len(), 1);
        let k4 = slab.insert(40);
        let k5 = slab.insert(50);
        assert_eq!(slab.get(k4), Some(&40));
        assert_eq!(slab.get(k5), Some(&50));
        slab.clear();
        assert_eq!(slab.len(), 0);
        assert_eq!(slab.get(k1), None);
        assert_eq!(slab.get(k2), None);
        assert_eq!(slab.get(k3), None);
        assert_eq!(slab.get(k4), None);
        assert_eq!(slab.get(k5), None);
    }

    #[test]
    fn test_retain(){
        let rand = &mut StdRng::seed_from_u64(96024);
        for len in successors(Some(2),|a|Some(a+rand.gen_range(5,10))).take(100).collect::<Vec<_>>() {
            let mut slab = ChunkSlab::<u16, _>::new();
            let mut entries = (0..len).map(|i|(i,slab.insert(i))).collect::<Vec<_>>();
            assert_eq!(entries.len(),len);
            assert_eq!(slab.len(),entries.len());
            let remove = entries.iter().copied().filter(|_|rand.gen_bool(0.7)).collect::<Vec<_>>();
            for e in &remove {
                let a = slab.remove(e.1).unwrap();
                let b = entries.remove(entries.iter().position(|v|e.1 == v.1).unwrap());
                assert_eq!(a,b.0);
            }
            assert_eq!(slab.len(),entries.len());
            slab.retain(|k,v|{
                if rand.gen_bool(0.5) {
                    let res = entries.remove(entries.iter().position(|v|v.1 == k).unwrap());
                    assert_eq!(res.0,*v);
                    false
                } else { true }
            });
            assert_eq!(slab.len(),entries.len());
            let slab = slab.iter().map(|(k,v)|(*v,k)).collect::<BTreeSet<_>>();
            assert_eq!(slab,entries.into_iter().collect::<BTreeSet<_>>());
        }
    }

    #[test]
    fn test_pinning() { //never realloc structures, required by scheduler algorithm
        let mut slab = ChunkSlab::<u8, _>::new();
        let k1 = slab.insert(10);
        let k2 = slab.insert(20);
        let ptr1 = slab.get(k1).unwrap() as *const _;
        let ptr2 = slab.get(k2).unwrap() as *const _;
        assert!(!core::ptr::eq(ptr1, ptr2));
        for _ in 0..200 {
            slab.insert(30);
        }
        let ptr11 = slab.get(k1).unwrap() as *const _;
        let ptr22 = slab.get(k2).unwrap() as *const _;
        assert!(core::ptr::eq(ptr1, ptr11));
        assert!(core::ptr::eq(ptr2, ptr22));
    }
}
