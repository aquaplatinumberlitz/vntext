//! A local web page that shows each line as it appears, for reading with
//! Yomitan. Lines are pushed to the browser with server-sent events.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const PAGE: &str = include_str!("page.html");
/// Events replayed to a browser that connects (or reconnects) later.
const HISTORY: usize = 200;

#[derive(Default)]
struct State {
    clients: Vec<Sender<String>>,
    history: VecDeque<String>,
}

#[derive(Clone, Default)]
pub struct Hub(Arc<Mutex<State>>);

impl Hub {
    /// Sends one JSON event to every connected page.
    pub fn publish(&self, event: String) {
        let mut state = self.0.lock().unwrap();
        state.clients.retain(|c| c.send(event.clone()).is_ok());
        state.history.push_back(event);
        if state.history.len() > HISTORY {
            state.history.pop_front();
        }
    }

    /// The events from now on, for a listener in this program.
    pub fn subscribe(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.0.lock().unwrap().clients.push(tx);
        rx
    }

    pub fn serve(&self, port: u16) -> std::io::Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let hub = self.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let hub = hub.clone();
                thread::spawn(move || {
                    let _ = hub.handle(stream);
                });
            }
        });
        Ok(())
    }

    fn handle(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let mut request = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut request)?;
        let path = request.split(' ').nth(1).unwrap_or("/");
        if !path.starts_with("/events") {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                PAGE.len()
            );
            stream.write_all(head.as_bytes())?;
            return stream.write_all(PAGE.as_bytes());
        }

        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n")?;
        let (tx, rx) = mpsc::channel();
        let backlog: Vec<String> = {
            let mut state = self.0.lock().unwrap();
            state.clients.push(tx);
            state.history.iter().cloned().collect()
        };
        for event in backlog {
            write!(stream, "data: {event}\n\n")?;
        }
        loop {
            match rx.recv_timeout(Duration::from_secs(15)) {
                Ok(event) => write!(stream, "data: {event}\n\n")?,
                // a comment line keeps the connection alive and reveals a closed tab
                Err(RecvTimeoutError::Timeout) => stream.write_all(b": ping\n\n")?,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            stream.flush()?;
        }
    }
}
