use super::{Task, TaskId};
use alloc::{collections::BTreeMap, sync::Arc};
use core::task::Waker;
use crossbeam_queue::ArrayQueue;

pub struct Executor {
    tasks: BTreeMap<TaskId, Task>,
    task_queue: Arc<ArrayQueue<TaskId>>,
    waker_cache: BTreeMap<TaskId, Waker>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            tasks: BTreeMap::new(),
            // wakers push IDs of awoken tasks onto the queue
            task_queue: Arc::new(ArrayQueue::new(100)),
            // no allocations here, because it's pushed to from an interrupt handler -- others are not
            // 100 is "small" for concurrent tasks, could be handled by distinct threads in Linux
            waker_cache: BTreeMap::new(),
        }
    }
}

impl Executor {
    pub fn spawn(&mut self, task: Task) {
        let task_id = task.id;
        if self.tasks.insert(task.id, task).is_some() {
            panic!("task with same ID already in tasks");
        }
        // interesting that this is a panic rather than an error --
        //  i guess because there is no return type?
        self.task_queue.push(task_id).expect("queue full");
        // pushing to the queue ensures that the future _will_ be polled
    }
}

use core::task::{Context, Poll};

struct TaskWaker {
    task_id: TaskId,
    task_queue: Arc<ArrayQueue<TaskId>>,
}
impl TaskWaker {
    fn wake_task(&self) {
        // no mut self because ArrayQueue is atomic
        self.task_queue.push(self.task_id).expect("task_queue full");
    }
}

impl Executor {
    fn run_ready_tasks(&mut self) {
        // destructure `self` to avoid borrow checker errors
        let Self {
            tasks,
            task_queue,
            waker_cache,
        } = self;

        while let Ok(task_id) = task_queue.pop() {
            let task = match tasks.get_mut(&task_id) {
                Some(task) => task,
                None => continue, // task no longer exists
            };
            let waker = waker_cache.entry(task_id).or_insert_with(|| {
                TaskWaker::new(
                    task_id,
                    task_queue.clone(), // waker has access to queue so we can push on wake
                ) // note: task_queue is an Arc, so .clone only increments the ref count
            });
            let mut context = Context::from_waker(waker);
            match task.poll(&mut context) {
                Poll::Ready(()) => {
                    // task done -> remove it and its cached waker
                    tasks.remove(&task_id);
                    waker_cache.remove(&task_id);
                }
                Poll::Pending => {}
            }
        }
    }
}

use alloc::task::Wake;

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_task();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wake_task();
    }
}

impl TaskWaker {
    fn new(task_id: TaskId, task_queue: Arc<ArrayQueue<TaskId>>) -> Waker {
        Waker::from(Arc::new(TaskWaker {
            task_id,
            task_queue,
        }))
    }
}

impl Executor {
    pub fn run(&mut self) -> ! {
        loop {
            self.run_ready_tasks();
            self.sleep_if_idle(); // prevent busy loop
        }
    }

    fn sleep_if_idle(&self) {
        use x86_64::instructions::interrupts::{self, enable_and_hlt};

        interrupts::disable(); // prevent race condition with interrupts that mutate queue
        if self.task_queue.is_empty() {
            enable_and_hlt(); // re-enable interrupts and issue a halt, atomically
        } else {
            interrupts::enable(); // re-enable interrupts and return
        }
    }
}
