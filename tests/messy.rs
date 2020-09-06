

mod common;
pub use common::*;
use juggle::*;
use rand::prelude::StdRng;
use rand::*;
use std::rc::Rc;
use rand::seq::SliceRandom;


fn generate_arbitrary_waits(handle: &WheelHandle<'_>,batch: usize,max_wait: usize,max_rep: usize,count: usize,rand: &mut StdRng){
    let batches = (0..count)
        .map(|_|(0..batch).map(|_|Signal::new()).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let batches = Rc::new(batches);
    let mut tasks = Vec::new();
    for (i,batch) in batches.iter().enumerate() {
        for (j,_) in batch.iter().enumerate() {
            let gen_before = rand.gen_range(0,max_wait);
            let gen_wait = rand.gen_range(0,max_wait);
            let gen_after = rand.gen_range(0,max_wait);
            let gen_repeats = rand.gen_range(0,max_rep);
            let batches = batches.clone();
            tasks.push(async move{
                Yield::times(gen_before).await;
                if i != 0 {
                    let sig = batches[i-1][j].clone();
                    sig.await; //wait for previous batch assignee
                }

                for _ in 0..gen_repeats{
                    Yield::times(gen_wait).await;
                    batches[i][j].signal(false); //dummy signal
                }
                batches[i][j].signal(true); //signal for next batch

                Yield::times(gen_after).await;
            });
        }
    }
    tasks.shuffle(rand);
    for t in tasks {
        handle.spawn_default(t).unwrap();
    }
}


#[test]
fn test_arbitrary_waits(){

    let rand = &mut StdRng::seed_from_u64(69420); //nice
    for _ in 0..200 {
        let wheel = Wheel::new();
        let batch = rand.gen_range(1,10);
        let wait = rand.gen_range(1,20);
        let reps = rand.gen_range(1,5);
        let count = rand.gen_range(1,100);
        generate_arbitrary_waits(wheel.handle(),batch,wait,reps,count,rand);
        smol::block_on(wheel).unwrap();
    }

}