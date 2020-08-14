use juggle::*;
use juggle::utils::{LoadBalance, TimerClock};
use std::rc::Rc;
use std::cell::Cell;
use rand::Rng;
use std::collections::HashMap;


struct MockClock(Rc<Cell<u64>>);

impl TimerClock for MockClock{
    type Duration = u64;
    type Instant = u64;
    fn start(&self) -> Self::Instant { self.0.get() }
    fn stop(&self, start: Self::Instant) -> Self::Duration { self.0.get() - start }
}

async fn load_task(index: usize,presets: &[(u16,fn(usize)->u64)],results: &[Cell<u64>],
                   clock: Rc<Cell<u64>>){
    loop{
        let time = presets[index].1(index);
        clock.set(clock.get() + time);
        results[index].set(results[index].get() + time);
        yield_once!();
    }
}
async fn control_task(handle: WheelHandle<'_>,to_cancel: Vec<IdNum>,clock: Rc<Cell<u64>>,total: u64){
    Yield::until(||clock.get() >= total).await;
    to_cancel.iter().for_each(|&id|{ handle.cancel(id); });
}

fn load_balance_tasks(total: u64,presets: &[(u16,fn(usize)->u64)])->Vec<u64>{
    let results = presets.iter().map(|_|Cell::new(0)).collect::<Vec<_>>();

    if let Some((first,rest)) = presets.split_first(){
        let clock = Rc::new(Cell::new(0));
        let wheel = Wheel::new();
        let handle = wheel.handle();
        let mut group = LoadBalance::with(MockClock(clock.clone()),first.0,load_task(0,presets,&results,clock.clone()));
        let mut ids = Vec::new();
        for (index,(prop,_)) in rest.iter().enumerate(){
            let index = index + 1; //cause first is already taken
            let id = handle.spawn(SpawnParams::default(),group.insert(*prop, load_task(index, presets, &results, clock.clone())));
            ids.push(id.unwrap());
        }
        let last = handle.spawn(SpawnParams::default(),group);
        ids.insert(0,last.unwrap());
        handle.spawn(SpawnParams::default(),control_task(handle.clone(),ids,clock,total));

        smol::block_on(wheel).unwrap();
    }
    results.iter().map(|a|a.get()).collect()
}

fn perform_test(presets: &[(u16,fn(usize)->u64)],time: u64){
    let results = load_balance_tasks(time,presets);
    let sum = results.iter().sum::<u64>();
    assert!(sum >= time);
    let mut map = HashMap::new();
    for (&v,(k,_)) in results.iter().zip(presets.iter()) {
        map.entry(k).or_insert(Vec::new()).push(v);
    }
    println!("sum: {} {:?} {:#?}",sum,results,map);
}

#[test]
fn test_static_balance(){
    let mut presets: Vec<(u16,fn(usize)->u64)> = Vec::new();
    presets.push((3,|_|{10}));
    presets.push((1,|_|{20}));
    presets.push((1,|_|{30}));
    presets.push((1,|_|{rand::thread_rng().gen_range(1,50)}));
    presets.push((3,|_|{11}));
    presets.push((1,|_|{21}));
    presets.push((1,|_|{31}));
    presets.push((1,|_|{rand::thread_rng().gen_range(1,60)}));
    presets.push((3,|_|{14}));
    presets.push((1,|_|{24}));
    presets.push((1,|_|{34}));
    presets.push((1,|_|{rand::thread_rng().gen_range(1,20)}));


    perform_test(&presets,5000);
}