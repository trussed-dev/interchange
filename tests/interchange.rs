#![cfg(not(loom))]

use interchange::Interchange;

#[test]
fn tests() {
    let interchange: Interchange<u64, u32, 10> = Interchange::new();
    let mut holder = Vec::new();
    for _ in 0..10 {
        holder.push(interchange.claim().unwrap());
    }

    assert!(interchange.claim().is_none());
    holder.clear();
    for _ in 0..10 {
        holder.push(interchange.claim().unwrap());
    }
    assert!(interchange.claim().is_none());

    holder.clear();
    for _ in 0..5 {
        holder.push(interchange.claim().unwrap());
    }
    holder.clear();
    for _ in 0..10 {
        holder.push(interchange.claim().unwrap());
    }
    assert!(interchange.claim().is_none());
}
