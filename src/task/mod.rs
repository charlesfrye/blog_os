use alloc::boxed::Box;
use core::{future::Future, pin::Pin};

pub mod keyboard;
pub mod simple_executor;

pub struct Task {
    // Future: Store Future trait implementors
    // dyn: These are dynamic, e.g. each async fn has a diff type
    // <Output = ()>: that do not return (aka return unit)
    // Box: Store those futures on the heap
    // Pin: Prevent &mut refs to futures so memory location stable
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + 'static) -> Task {
        Task {
            future: Box::pin(future),
        }
    }
}

use core::task::{Context, Poll};

impl Task {
    fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}
