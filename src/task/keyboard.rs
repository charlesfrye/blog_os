use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;

// using OnceCell to get one-time heap allocation
// preferred to lazy_static because we can guarantee
// allocation occurs outside of interrupt handler
static SCANCODE_QUEUE: // a statically-sized queue for holding scancodes
    OnceCell<ArrayQueue<u8>> = OnceCell::uninit();

use crate::println;

use futures_util::task::AtomicWaker;
static WAKER: AtomicWaker = AtomicWaker::new();

/// Called by the keyboard interrupt handler
/// So it must not block or allocate.
/// Instead, it spins -- I think around a compare-and-swap instruction?
pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if queue.push(scancode).is_err() {
            println!("WARNING: scancode queue full; dropping keyboard input");
        } else {
            WAKER.wake(); // this calls the parent task's .wake
        }
    } else {
        println!("WARNING: scancode queue uninitialized");
    }
}

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE
            .try_init_once(|| ArrayQueue::new(100))
            .expect("ScancodeStream::new should only be called once");
        ScancodeStream { _private: () }
    }
}

impl Default for ScancodeStream {
    fn default() -> Self {
        Self::new()
    }
}

use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures_util::stream::Stream;

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE
            .try_get()
            .expect("scancode queue not initialized");

        // fast path
        if let Ok(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        // overwrite our waker with the parent task's waker
        // -- it's our duty if we return Poll::Pending
        WAKER.register(cx.waker());
        match queue.pop() {
            Ok(scancode) => {
                // if interrupt pushed since we last checked
                WAKER.take(); // nvm, we don't need that waker
                Poll::Ready(Some(scancode))
            }
            Err(crossbeam_queue::PopError) => Poll::Pending,
        }
    }
}

use crate::print;
use futures_util::stream::StreamExt;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

pub async fn print_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore);

    while let Some(scancode) = scancodes.next(
        // async/await sugar means parent context/waker automatically injected here
    ).await
    {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => print!("{}", character),
                    DecodedKey::RawKey(key) => print!("{:?}", key),
                }
            }
        }
    }
}
