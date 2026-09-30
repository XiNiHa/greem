//! THROWAWAY alternative: inspectable owner/dependent frames.
//! All handwritten code is safe. `self_cell` supplies the unsafe implementation
//! of the owner/dependent container, including dependent-before-owner drop.
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

#[derive(Debug, Serialize)]
pub struct Record<'a> {
    pub group: usize,
    pub index: usize,
    pub parent: &'a str,
    pub value: &'a str,
    pub deferred: bool,
}

#[derive(Debug, Serialize)]
pub struct Payload<'a> {
    pub barrier: usize,
    pub records: Vec<Record<'a>>,
}

// The generated adapter projects only Sync objects into Send futures. The
// owner batch itself need only be Send, as in the accepted Outputs prototype.
pub trait Batch: Send {
    fn start(&self) -> Box<dyn Chain + '_>;
}

pub trait Chain: Send {
    fn poll_generation(&mut self, cx: &mut Context<'_>) -> Poll<()>;
    fn records<'view>(&'view self, output: &mut Vec<Record<'view>>);
    // Called only after the synchronous sink returns; true means retire.
    fn advance(&mut self) -> bool;
    fn frame_depth(&self) -> usize;
}

type Work<'a> = Box<dyn Chain + 'a>;

self_cell::self_cell!(
    pub struct Frame<'owner> {
        owner: Box<dyn Batch + 'owner>,
        #[not_covariant]
        dependent: Work,
    }
);

impl<'owner> Frame<'owner> {
    pub fn from_batch(owner: Box<dyn Batch + 'owner>) -> Self {
        Self::new(owner, |owner| owner.start())
    }
}

impl Chain for Frame<'_> {
    fn poll_generation(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.with_dependent_mut(|_, work| work.poll_generation(cx))
    }
    fn records<'view>(&'view self, output: &mut Vec<Record<'view>>) {
        self.with_dependent(|_, work| work.records(output));
    }
    fn advance(&mut self) -> bool {
        self.with_dependent_mut(|_, work| work.advance())
    }
    fn frame_depth(&self) -> usize {
        1 + self.with_dependent(|_, work| work.frame_depth())
    }
}

pub async fn execute(
    mut root: Frame<'_>,
    mut sink: impl for<'payload> FnMut(Payload<'payload>) + Send,
) {
    let mut barrier = 0;
    loop {
        futures::future::poll_fn(|cx| root.poll_generation(cx)).await;
        {
            let mut records = Vec::new();
            root.records(&mut records);
            // A single structured view over all ready chains. No encoded
            // fragments, JSON Value tree, cloned strings or response registry.
            sink(Payload { barrier, records });
        }
        if root.advance() {
            break;
        }
        barrier += 1;
    }
}

// Shared counters are evidence, not the ownership mechanism.
#[derive(Default)]
pub struct Evidence {
    pub log: Vec<String>,
    pub live_turns: usize,
    pub max_live_turns: usize,
    pub max_depth: usize,
    pub polls: usize,
}
pub type Log = Arc<Mutex<Evidence>>;
