//! Persistent PipeWire output. HTTP reads never wait for audible playout.
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc::{self, SyncSender}};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct Player {
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    until: Arc<Mutex<Instant>>,
}
impl Player {
    pub fn new(volume: f64) -> Result<Self, String> {
        let parent = std::process::id();
        let mut command = Command::new("pw-play");
        command.args(["--raw", "--rate=48000", "--channels=1", "--format=s16", "--latency=80ms"])
            .arg(format!("--volume={volume}")).arg("-").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 { return Err(std::io::Error::last_os_error()); }
                if libc::getppid() as u32 != parent { return Err(std::io::Error::other("reader exited")); }
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|_| "Không mở được pw-play. Kiểm tra pipewire-audio.".to_string())?;
        Ok(Self { input: Arc::new(Mutex::new(child.stdin.take().unwrap())), child,
            done: Arc::new(AtomicBool::new(true)), failed: Arc::new(AtomicBool::new(false)), until: Arc::new(Mutex::new(Instant::now())) })
    }
    pub fn begin(&self) -> SyncSender<Vec<u8>> {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(256);
        self.done.store(false, Ordering::Release);
        self.failed.store(false, Ordering::Release);
        *self.until.lock().unwrap() = Instant::now();
        let (input, done, failed, until) = (self.input.clone(), self.done.clone(), self.failed.clone(), self.until.clone());
        std::thread::spawn(move || {
            for data in rx {
                let began = Instant::now();
                if input.lock().unwrap().write_all(&data).is_err() {
                    failed.store(true, Ordering::Release); break;
                }
                let mut end = until.lock().unwrap();
                *end = (*end).max(began) + Duration::from_secs_f64(data.len() as f64 / 96000.0);
            }
            done.store(true, Ordering::Release);
        });
        tx
    }
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Acquire) && Instant::now() >= *self.until.lock().unwrap() + Duration::from_millis(100)
    }
    #[cfg(test)]
    pub fn id(&self) -> u32 { self.child.id() }
    pub fn failed(&mut self) -> bool {
        self.failed.load(Ordering::Acquire) || self.child.try_wait().map_or(true, |exit| exit.is_some())
    }
}
impl Drop for Player {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
