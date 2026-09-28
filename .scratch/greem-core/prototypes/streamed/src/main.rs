//! THROWAWAY downstream application: real owner borrows, no cloned leaf values.
#![forbid(unsafe_code)]
use futures::{
    future::{BoxFuture, poll_fn},
    stream::{FuturesUnordered, StreamExt},
    task::noop_waker,
};
use greem_streamed_probe::Barrier;
use serde::Serialize;
use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

struct Parent {
    name: String,
    log: Arc<Mutex<Vec<String>>>,
}
impl Drop for Parent {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push(format!("parent:{}", self.name));
    }
}
struct Item<'a> {
    parent: &'a Parent,
    text: String,
}
impl Drop for Item<'_> {
    fn drop(&mut self) {
        self.parent
            .log
            .lock()
            .unwrap()
            .push(format!("item:{}:{}", self.parent.name, self.text));
    }
}
struct Child<'a>(&'a Item<'a>);
impl Drop for Child<'_> {
    fn drop(&mut self) {
        self.0
            .parent
            .log
            .lock()
            .unwrap()
            .push(format!("child:{}:{}", self.0.parent.name, self.0.text));
    }
}
#[derive(Serialize)]
struct Fragment<'a> {
    parent: &'a str,
    item: &'a str,
    deferred: bool,
}

fn request(barrier: Barrier, log: Arc<Mutex<Vec<String>>>, cancel: bool) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let parents = [
            Parent {
                name: "Ada".into(),
                log: log.clone(),
            },
            Parent {
                name: "Lin".into(),
                log: log.clone(),
            },
        ];
        // Two sibling chains borrow the same containing frame. Registration is
        // eager, before polling either sibling, so the live set is well-defined.
        let mut turns: FuturesUnordered<BoxFuture<'_, ()>> = FuturesUnordered::new();
        for parent in &parents {
            let participant = barrier.register();
            turns.push(Box::pin(async move {
                let item = Item {
                    parent,
                    text: format!("{}'s item", parent.name),
                };
                let child = Child(&item);
                // One slow child: real Pending plus wake, no sleeps or spawning.
                let mut yielded = false;
                poll_fn(|cx| {
                    if cancel {
                        return Poll::<()>::Pending;
                    }
                    if parent.name == "Lin" && !yielded {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(())
                    }
                })
                .await;
                participant
                    .publish(&Fragment {
                        parent: &child.0.parent.name,
                        item: &child.0.text,
                        deferred: false,
                    })
                    .await;
                // Deferred data still borrows the turn's owned item after its
                // first payload ships. Both owners survive the second barrier.
                participant
                    .publish(&Fragment {
                        parent: &child.0.parent.name,
                        item: &child.0.text,
                        deferred: true,
                    })
                    .await;
            }));
        }
        while turns.next().await.is_some() {}
    })
}

fn main() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let barrier = Barrier::default();
    let mut payloads = Vec::new();
    futures::executor::block_on(barrier.drive(
        request(barrier.clone(), log.clone(), false),
        |bytes| {
            assert!(
                log.lock().unwrap().is_empty(),
                "all owners must survive both payloads"
            );
            payloads.push(serde_json::from_str::<serde_json::Value>(&bytes).unwrap());
            println!("{bytes}");
        },
    ));
    assert_eq!(payloads.len(), 2);
    for (epoch, payload) in payloads.iter().enumerate() {
        assert_eq!(payload["barrier"], epoch);
        assert_eq!(payload["fragments"].as_array().unwrap().len(), 2);
        assert_eq!(payload["fragments"][0]["deferred"], epoch == 1);
    }
    for parent in ["Ada", "Lin"] {
        let log = log.lock().unwrap();
        let child = log
            .iter()
            .position(|s| s == &format!("child:{parent}:{parent}'s item"))
            .unwrap();
        let item = log
            .iter()
            .position(|s| s == &format!("item:{parent}:{parent}'s item"))
            .unwrap();
        let root = log
            .iter()
            .position(|s| s == &format!("parent:{parent}"))
            .unwrap();
        assert!(child < item && item < root);
    }
    println!("normal destruction: {:?}", log.lock().unwrap());

    let log = Arc::new(Mutex::new(Vec::new()));
    let barrier = Barrier::default();
    let mut cancelled = Box::pin(barrier.drive(
        request(barrier.clone(), log.clone(), true),
        |_| {
            panic!("cancellation must not emit");
        },
    ));
    let waker = noop_waker();
    assert!(
        cancelled
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    drop(cancelled);
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 6);
    for parent in ["Ada", "Lin"] {
        let child = log
            .iter()
            .position(|s| s.starts_with(&format!("child:{parent}:")))
            .unwrap();
        let item = log
            .iter()
            .position(|s| s.starts_with(&format!("item:{parent}:")))
            .unwrap();
        let root = log
            .iter()
            .position(|s| s == &format!("parent:{parent}"))
            .unwrap();
        assert!(child < item && item < root);
    }
    println!("cancelled destruction: {log:?}");
}
