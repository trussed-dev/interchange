#![cfg(loom)]

use loom::thread;
use std::mem::drop;

use interchange::{Channel, Requester, Responder};
#[allow(deprecated)]
use std::sync::atomic::{
    AtomicBool,
    Ordering::{Acquire, Release},
    ATOMIC_BOOL_INIT,
};

#[allow(deprecated)]
static BRANCHES_USED: [AtomicBool; 6] = [ATOMIC_BOOL_INIT; 6];

#[test]
fn loom_interchange() {
    loom::model(|| {
        let interchange = Box::leak(Box::new(Channel::new()));
        let dropper = unsafe { Box::from_raw(interchange as _) };

        let (rq, rp) = unsafe { interchange.split() };
        let handle1 = thread::spawn(move || requester_thread(rq));
        let handle2 = thread::spawn(move || responder_thread(rp));
        let res1 = handle1.join();
        let res2 = handle2.join();

        // Avoid memory leak
        drop(dropper);

        res1.unwrap();
        res2.unwrap();
    });

    // Verify that the model explored the expected branches
    for b in &BRANCHES_USED {
        assert!(b.load(Acquire));
    }
}

fn requester_thread(mut requester: Requester<'static, u64, u64>) -> Option<()> {
    requester.request(&53).unwrap();
    requester.with_response(|r| assert_eq!(*r, 63)).ok()?;
    requester.with_response(|r| assert_eq!(*r, 63)).ok()?;
    requester.take_response().unwrap();
    requester.with_request_mut(|r| *r = 51).unwrap();
    requester.send_request().unwrap();
    thread::yield_now();
    match requester.cancel() {
        Ok(Some(51) | None) => BRANCHES_USED[0].store(true, Release),
        Ok(_) => panic!("Invalid state"),
        Err(_) => {
            BRANCHES_USED[1].store(true, Release);
            assert_eq!(requester.take_response().unwrap(), 79);
        }
    }
    BRANCHES_USED[4].store(true, Release);
    None
}

fn responder_thread(mut responder: Responder<'static, u64, u64>) -> Option<()> {
    let req = responder.take_request()?;
    assert_eq!(req, 53);
    responder.respond(&(req + 10)).unwrap();
    thread::yield_now();
    responder
        .with_request(|r| {
            BRANCHES_USED[2].store(true, Release);
            assert_eq!(*r, 51)
        })
        .map(|_| assert!(responder.with_request(|_| {}).is_err()))
        .or_else(|_| {
            BRANCHES_USED[3].store(true, Release);
            responder.acknowledge_cancel()
        })
        .ok()?;
    responder.with_response_mut(|r| *r = 79).ok();
    responder.send_response().ok();
    BRANCHES_USED[5].store(true, Release);
    None
}
