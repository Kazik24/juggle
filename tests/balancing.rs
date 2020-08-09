use juggle::*;
use juggle::utils::{LoadBalance, TimerClock};
use std::time::{Instant, Duration};
use std::thread::{sleep, spawn};
use rand::Rng;
use std::collections::{HashMap, BTreeMap};
use std::cell::Cell;

fn long_operation(milis: u64)->Duration{
    let start = Instant::now();
    // for _ in 0..milis {
    //     std::thread::yield_now();
    // }
    sleep(Duration::from_millis(milis));
    Instant::now() - start
}

async fn load_task(index: usize,presets: &[(u16,fn(usize)->u64)],results: &[Cell<Duration>]){
    loop{
        let time = long_operation(presets[index].1(index));
        let v = results[index].get();
        results[index].set(v + time);
        yield_once!();
    }
}

async fn control_task(handle: WheelHandle<'_>,to_cancel: Vec<IdNum>,total: u64){
    let start = Instant::now();
    yield_until!(Instant::now() - start >= Duration::from_millis(total));
    to_cancel.iter().for_each(|&id|{ handle.cancel(id); });
}

fn create_clock()->impl TimerClock {
    struct MockClock;
    impl TimerClock for MockClock{
        type Duration = Duration;
        type Instant = Instant;
        fn start(&self) -> Self::Instant { Instant::now() }
        fn stop(&self, start: Self::Instant) -> Self::Duration { Instant::now() - start }
    }
    #[cfg(feature="std")]
    return juggle::utils::StdTimerClock;
    #[cfg(not(feature="std"))]
    return MockClock;
}

fn load_balance_tasks(total: u64,presets: &[(u16,fn(usize)->u64)])->Vec<u64>{
    let results = presets.iter().map(|_|Cell::new(Duration::default())).collect::<Vec<_>>();

    if let Some((first,rest)) = presets.split_first(){
        let wheel = Wheel::new();
        let handle = wheel.handle();
        let mut group = LoadBalance::with(create_clock(), first.0, load_task(0, presets, &results));
        let mut ids = Vec::new();
        for (index,(prop,_)) in rest.iter().enumerate(){
            let index = index + 1; //cause first is already taken
            let id = handle.spawn(SpawnParams::default(),group.add(*prop,load_task(index,presets,&results)));
            ids.push(id.unwrap());
        }
        let last = handle.spawn(SpawnParams::default(),group);
        ids.insert(0,last.unwrap());
        handle.spawn(SpawnParams::default(),control_task(handle.clone(),ids,total));

        smol::block_on(wheel).unwrap();
    }
    results.iter().map(|a|a.get().as_millis() as u64).collect()
}



fn calc_avg_std(arr: &[u64])->(i64,u64){
    let avg = (arr.iter().sum::<u64>() / arr.len() as u64) as i64;
    let sqsum = arr.iter().map(|&v|{
        let v = v as i64 - avg;
        v*v
    }).sum::<i64>() as f64;
    let std = if arr.len() <= 1 { 0 } else { (sqsum / (arr.len() - 1) as f64).sqrt() as u64 };
    (avg,std)
}

fn filter_results(presets: &[(u16,fn(usize)->u64)],results: &[u64])->HashMap<u16,Vec<u64>>{
    let mut map = HashMap::new();
    for (&p,&r) in presets.iter().zip(results.iter()){
        let arr = map.entry(p.0).or_insert(Vec::new());
        arr.push(r);
    }
    map
}
fn combine_results(res: &[HashMap<u16,Vec<u64>>])->HashMap<u16,Vec<u64>>{
    let mut map = HashMap::new();
    for m in res {
        for (&k,v) in m.iter() {
            map.entry(k).or_insert(Vec::new()).extend(v);
        }
    }
    map
    // let mut processed = BTreeMap::new();
    // for (k,v) in map {
    //     let (avg,std) = calc_avg_std(&v);
    //     processed.insert(k,(avg,std,v));
    // }
    // processed
}

#[allow(dead_code)]
fn print_results(map: &BTreeMap<u16,(u64,u64,Vec<u64>)>,sigma: f64){
    for (&k,v) in map.iter(){
        let deviation = v.1 as f64 * sigma;
        let p = if v.0 <= 0 { 0.0 } else { deviation / v.0 as f64 };
        let params = format!("avg={} ±{:.1}%",v.0,p*100.0);
        println!("P={:>3}, {:>20}, {:?}",k,params,v.2);
    }
}

fn perform_test(presets: &[(u16,fn(usize)->u64)],sigma: f64,threads: usize,time_ms: usize){
    let handles = (0..threads).map(|_|{
        let p = presets.to_vec();
        spawn(move||load_balance_tasks(5000,&p))
    }).collect::<Vec<_>>();
    let results = handles.into_iter().map(|h|filter_results(&presets,&h.join().unwrap())).collect::<Vec<_>>();
    let results = combine_results(&results);

    //calculate ideal values
    let sum: usize = results.iter().map(|(&k,v)| k as usize * (v.len() / threads)).sum();
    let time_slice = time_ms as f64 / sum as f64;
    for (&k,v) in results.iter() {
        let expect = if v.is_empty() { 0 } else { (k as f64 * time_slice) as i64 };
        let (avg,std) = calc_avg_std(v);
        let max = std as f64 * sigma;
        let error = (avg - expect).abs() as f64;
        println!("P={:>2}, expect={:>4}, avg={:>4} ± {:<4.0}, err={:>4.0}",k,expect,avg,max,error);
        assert!(error <= max);
    }
}


#[test]
fn test_load_balancing(){
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


    perform_test(&presets,5.0,200,5000);
    //print_results(&results,5.0);

}

