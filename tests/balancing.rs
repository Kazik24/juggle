use juggle::*;
use std::sync::atomic::*;
use std::time::{Instant, Duration};
use std::thread::sleep;
use rand::Rng;

fn long_operation(milis: u64)->u64{
    let start = Instant::now();
    sleep(Duration::from_millis(milis));
    let dur = Instant::now() - start;
    dur.as_millis() as u64
}

async fn load_task(index: usize,presets: &[(u8,fn(usize)->u64)],results: &[AtomicU64]){
    loop{
        let time = long_operation(presets[index].1(index));
        results[index].fetch_add(time,Ordering::Relaxed);
        yield_once!();
    }
}

async fn control_task(handle: WheelHandle<'_>,to_cancel: Vec<IdNum>,total: u64){
    let start = Instant::now();
    yield_until!(Instant::now() - start >= Duration::from_millis(total));
    to_cancel.iter().for_each(|&id|{ handle.cancel(id); });
}

fn load_balance_tasks(total: u64,presets: &[(u8,fn(usize)->u64)])->Vec<u64>{
    let results = presets.iter().map(|_|AtomicU64::new(0)).collect::<Vec<_>>();

    if let Some((first,rest)) = presets.split_first(){
        let wheel = Wheel::new();
        let handle = wheel.handle();
        let mut group = LoadBalance::with(first.0,load_task(0,presets,&results));
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
    results.iter().map(|a|a.load(Ordering::Relaxed)).collect()
}


#[test]
fn test_load_balancing(){
    let mut presets: Vec<(u8,fn(usize)->u64)> = Vec::new();
    presets.push((3,|_|{10}));
    presets.push((1,|_|{20}));
    presets.push((1,|_|{30}));
    presets.push((1,|_|{rand::thread_rng().gen_range(1,50)}));

    let result = load_balance_tasks(2000,&presets);

    println!("Times: {:?}",result);
}

