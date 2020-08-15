use std::cell::*;
use std::collections::*;
use std::ops::Range;
use std::rc::Rc;
use rand::*;
use rand::prelude::StdRng;
use juggle::*;
use juggle::utils::*;

struct MockClock(Rc<Cell<u64>>);

impl TimerClock for MockClock {
    type Duration = u64;
    type Instant = u64;
    fn start(&self) -> Self::Instant { self.0.get() }
    fn stop(&self, start: Self::Instant) -> Self::Duration { self.0.get() - start }
}

async fn load_task(index: usize, presets: &[(u16, Box<dyn Fn(usize) -> u64>)], results: &[Cell<u64>],
                   clock: Rc<Cell<u64>>) {
    loop {
        let time = presets[index].1(index);
        clock.set(clock.get() + time);
        results[index].set(results[index].get() + time);
        yield_once!();
    }
}

async fn control_task(handle: WheelHandle<'_>, to_cancel: Vec<IdNum>, clock: Rc<Cell<u64>>, total: u64) {
    Yield::until(|| clock.get() >= total).await;
    to_cancel.into_iter().for_each(|id| { handle.cancel(id); });
}

fn load_balance_tasks(total: u64, presets: &[(u16, Box<dyn Fn(usize) -> u64>)]) -> Vec<u64> {
    let results = presets.iter().map(|_| Cell::new(0)).collect::<Vec<_>>();

    if let Some((first, rest)) = presets.split_first() {
        let clock = Rc::new(Cell::new(0));
        let wheel = Wheel::new();
        let handle = wheel.handle();
        let group = LoadBalance::with(MockClock(clock.clone()), first.0, load_task(0, presets, &results, clock.clone()));
        let mut ids = Vec::new();
        for (index, (prop, _)) in rest.iter().enumerate() {
            let index = index + 1; //cause first is already taken
            let id = handle.spawn(SpawnParams::default(), group.insert(*prop, load_task(index, presets, &results, clock.clone())));
            ids.push(id.unwrap());
        }
        let last = handle.spawn(SpawnParams::default(), group);
        ids.insert(0, last.unwrap());
        handle.spawn(SpawnParams::default(), control_task(handle.clone(), ids, clock, total));

        smol::block_on(wheel).unwrap();
    }
    results.iter().map(|a| a.get()).collect()
}

fn perform_test(presets: &[(u16, Box<dyn Fn(usize) -> u64>)], time: u64) {
    let results = load_balance_tasks(time, presets);
    let sum = results.iter().sum::<u64>();
    assert!(sum >= time);
    let mut map = BTreeMap::new();
    for (i, (&v, (k, _))) in results.iter().zip(presets.iter()).enumerate() {
        map.entry(*k).or_insert(Vec::new()).push((v, i));
    }

    let sum_slots: u64 = map.iter().map(|(&k, v)| k as u64 * v.len() as u64).sum();
    let time_slice = time as f64 / sum_slots as f64;
    for (&k, v) in map.iter() {
        let expect = if v.is_empty() { 0.0 } else { k as f64 * time_slice };
        for e in v {
            let range = [(expect * 0.8) as _, (expect * 1.2) as _];
            assert!(e.0 >= range[0] && e.0 <= range[1] as _,
                    "Preset failed [{}]: expect range: {:?}, got: {}, slots: {}", e.1, range, e.0, k);
        }
    }
    //println!("sum: {} {:?} {:#?}",sum,results,map);
}


fn rand(slots: u16, range: Range<u64>, rng: &Rc<RefCell<StdRng>>) -> (u16, Box<dyn Fn(usize) -> u64>) {
    let rng = rng.clone();
    (slots, Box::new(move |_| rng.borrow_mut().gen_range(range.start, range.end)))
}

fn exact(slots: u16, val: u64) -> (u16, Box<dyn Fn(usize) -> u64>) {
    (slots, Box::new(move |_| val))
}

#[test]
fn test_static_balance_simple() {
    let rng = &Rc::new(RefCell::new(StdRng::seed_from_u64(12345)));

    let mut presets = Vec::new();
    presets.push(exact(3, 10));
    presets.push(exact(1, 20));
    presets.push(exact(1, 30));
    presets.push(rand(1, 1..50, rng));
    presets.push(exact(3, 11));
    presets.push(exact(1, 21));
    presets.push(exact(1, 31));
    presets.push(rand(1, 1..60, rng));
    presets.push(exact(3, 14));
    presets.push(exact(1, 24));
    presets.push(exact(1, 34));
    presets.push(rand(1, 1..20, rng));


    perform_test(&presets, 5000);
}

#[test]
fn test_static_balance_many() {
    let rng = &Rc::new(RefCell::new(StdRng::seed_from_u64(56789)));

    let mut presets = Vec::new();

    for _ in 0..200 {
        if rng.borrow_mut().gen_bool(0.2) {
            presets.push(rand(rng.borrow_mut().gen_range(1, 10), 1..20, rng));
        } else {
            let mut rng = rng.borrow_mut();
            presets.push(exact(rng.gen_range(1, 20), rng.gen_range(10, 20)));
        }
    }

    perform_test(&presets, 500000);
}