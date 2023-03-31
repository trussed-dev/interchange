#[cfg(loom)]
use loom::thread;
#[cfg(not(loom))]
use std::thread;

use std::mem::drop;

use interchange::{Channel, Requester, Responder};
#[cfg(loom)]
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::{AtomicBool, Ordering::Release};

static BRANCHES_USED: [AtomicBool; 6] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const ATOMIC_BOOL_INIT: AtomicBool = AtomicBool::new(false);
    [ATOMIC_BOOL_INIT; 6]
};

#[cfg(loom)]
#[test]
fn loom_interchange() {
    loom::model(test_function);

    // Verify that the model explored the expected branches
    for b in &BRANCHES_USED {
        assert!(b.load(Acquire));
    }
}

// This is tested even with the standard library to ensure that the Send/Sync traits are implemented as necessary
// Loom's thread::spawn doesn't require the function to be `Send`
#[cfg_attr(not(loom), test)]
fn test_function() {
    // thread closures must be 'static
    let channel = Box::leak(Box::new(Channel::new()));
    let dropper = unsafe { Box::from_raw(channel as _) };

    let (rq, rp) = channel.split().unwrap();

    let handle1 = thread::spawn(move || requester_thread(rq));
    let handle2 = thread::spawn(move || responder_thread(rp));
    let res1 = handle1.join();
    let res2 = handle2.join();

    // Avoid memory leak
    drop(dropper);

    res1.unwrap();
    res2.unwrap();
}

fn requester_thread(mut requester: Requester<'static, u64, u64>) -> Option<()> {
    requester.request(53).unwrap();
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
    responder.respond(req + 10).unwrap();
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
