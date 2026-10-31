use crate::backend::{self, Prepared, Spec};
use quicklook_core::{config::Config, files::FileInfo};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

pub enum Task {
    Prepare(FileInfo, Spec),
    Extract(FileInfo, usize),
    DirectoryStats(FileInfo),
    Tile(FileInfo, [u32; 4]),
}
struct Job {
    id: u64,
    task: Task,
    cancel: Arc<AtomicBool>,
}
pub struct Delivery {
    pub id: u64,
    pub result: Result<Prepared, String>,
}
#[derive(Default)]
struct Queue {
    high: VecDeque<Job>,
    low: VecDeque<Job>,
    shutdown: bool,
}
pub struct Workers {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    pub receiver: mpsc::Receiver<Delivery>,
}
impl Workers {
    pub fn new(config: Config) -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (sender, receiver) = mpsc::channel();
        for index in 0..2 {
            let queue = queue.clone();
            let sender = sender.clone();
            let config = config.clone();
            std::thread::Builder::new()
                .name(format!("quicklook-decode-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let (lock, notify) = &*queue;
                            let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                            while queue.high.is_empty() && queue.low.is_empty() && !queue.shutdown {
                                queue = notify.wait(queue).unwrap_or_else(|e| e.into_inner());
                            }
                            if queue.shutdown {
                                return;
                            }
                            queue
                                .high
                                .pop_front()
                                .or_else(|| queue.low.pop_front())
                                .unwrap()
                        };
                        if job.cancel.load(Ordering::Relaxed) {
                            let _ = sender.send(Delivery {
                                id: job.id,
                                result: Err("已取消".into()),
                            });
                            continue;
                        }
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            match job.task {
                                Task::Prepare(info, spec) => {
                                    backend::prepare(info, &spec, &config, &job.cancel)
                                }
                                Task::Extract(info, index) => {
                                    backend::extract(&info, index, &config, &job.cancel)
                                }
                                Task::DirectoryStats(info) => {
                                    backend::directory_stats(info, &job.cancel)
                                }
                                Task::Tile(info, rect) => {
                                    crate::native::region(&info.path, rect, 2048, &job.cancel).map(
                                        |pixels| Prepared {
                                            info,
                                            body: backend::Body::Tile { pixels, rect },
                                            note: String::new(),
                                            temporary: None,
                                        },
                                    )
                                }
                            }
                        }))
                        .map_err(|_| "预览后端异常".to_string())
                        .and_then(|r| r.map_err(|e| format!("{e:#}")));
                        if sender.send(Delivery { id: job.id, result }).is_err() {
                            return;
                        }
                    }
                })
                .expect("无法创建解码线程");
        }
        Self { queue, receiver }
    }
    pub fn submit(&self, id: u64, task: Task, cancel: Arc<AtomicBool>, low: bool) -> bool {
        let (lock, notify) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        queue.high.retain(|j| !j.cancel.load(Ordering::Relaxed));
        queue.low.retain(|j| !j.cancel.load(Ordering::Relaxed));
        if low && queue.low.len() >= 48 {
            return false;
        }
        let job = Job { id, task, cancel };
        if low {
            queue.low.push_back(job);
        } else {
            queue.high.push_back(job);
        }
        notify.notify_one();
        true
    }
    pub fn cancel_prefetch(&self) {
        let (mut queue, _) = (
            self.queue.0.lock().unwrap_or_else(|e| e.into_inner()),
            &self.queue.1,
        );
        for job in queue.low.drain(..) {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        let (lock, notify) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        queue.shutdown = true;
        for job in queue.high.drain(..) {
            job.cancel.store(true, Ordering::Relaxed);
        }
        for job in queue.low.drain(..) {
            job.cancel.store(true, Ordering::Relaxed);
        }
        notify.notify_all();
    }
}
