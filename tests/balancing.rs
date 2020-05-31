use juggle::*;
use std::sync::atomic::*;
use std::time::{Instant, Duration};
use std::thread::sleep;
use rand::Rng;

static TASK1_MS: AtomicU64 = AtomicU64::new(0);
static TASK2_MS: AtomicU64 = AtomicU64::new(0);
static TASK3_MS: AtomicU64 = AtomicU64::new(0);
static TASK4_MS: AtomicU64 = AtomicU64::new(0);

fn long_operation(milis: u64)->u64{
    let start = Instant::now();
    sleep(Duration::from_millis(milis));
    let dur = Instant::now() - start;
    dur.as_millis() as u64
}

async fn task1(){
    loop{
        let time = long_operation(10);
        TASK1_MS.fetch_add(time,Ordering::AcqRel);
        yield_once!();
    }
}
async fn task2(){
    loop{
        let time = long_operation(20);
        TASK2_MS.fetch_add(time,Ordering::AcqRel);
        yield_once!();
    }
}
async fn task3(){
    loop{
        let time = long_operation(40);
        TASK3_MS.fetch_add(time,Ordering::AcqRel);
        yield_once!();
    }
}
async fn task4(){
    loop{
        let time = long_operation(rand::thread_rng().gen_range(1,50));
        TASK4_MS.fetch_add(time,Ordering::AcqRel);
        yield_once!();
    }
}

async fn control(handle: WheelHandle,ids: [IdNum;4]){
    let start = Instant::now();
    yield_until!(Instant::now() - start >= Duration::from_millis(2000));
    ids.iter().for_each(|&id|{ handle.cancel(id); });
}


#[test]
fn test_load_balancing(){
    let wheel = Wheel::new();
    let handle = wheel.handle();

    let mut task1_load = LoadBalance::with(1,task1());
    let t2 = handle.spawn(SpawnParams::default(),task1_load.add(1,task2())).unwrap();
    let t3 = handle.spawn(SpawnParams::default(),task1_load.add(1,task3())).unwrap();
    let t4 = handle.spawn(SpawnParams::default(),task1_load.add(1,task4())).unwrap();
    let t1 = handle.spawn(SpawnParams::default(),task1_load).unwrap();
    handle.spawn(SpawnParams::default(),control(handle.clone(),[t1,t2,t3,t4])).unwrap();

    smol::block_on(wheel);

    let times = [TASK1_MS.load(Ordering::Relaxed),
                 TASK2_MS.load(Ordering::Relaxed),
                 TASK3_MS.load(Ordering::Relaxed),
                 TASK4_MS.load(Ordering::Relaxed)];
    println!("Times: {:?}",times);
}

