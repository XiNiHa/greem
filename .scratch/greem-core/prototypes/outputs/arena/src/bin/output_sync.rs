#![allow(unexpected_cfgs)]
use std::cell::Cell;
struct Output { text: String, marker: Cell<u32> }
fn main() {
    let run = async {
        // Output is Send, not Sync. The frame owns it across the child await.
        let outputs = vec![Output { text: "child".to_owned(), marker: Cell::new(0) }];
        #[cfg(not(whole_output))]
        let child_refs: Vec<_> = outputs.iter().map(|value| value.text.as_str()).collect();
        #[cfg(whole_output)]
        let child_refs: Vec<_> = outputs.iter().collect();
        let children = child_refs.into_iter().map(|child| async move {
            futures::future::ready(()).await;
            #[cfg(not(whole_output))]
            assert_eq!(child, "child");
            #[cfg(whole_output)]
            assert_eq!(child.text, "child");
        });
        futures::future::join_all(children).await;
        assert_eq!(outputs[0].marker.get(), 0);
    };
    fn assert_send<T: Send>(_: &T) {}
    assert_send(&run);
    futures::executor::block_on(run);
    println!("Send-only output owner retained; Sync child projection shared across await");
}
