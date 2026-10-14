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
    messages: u64,
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

    /// The number of the next message, whoever shows it.
    pub fn next_seq(&self) -> u64 {
        let mut state = self.0.lock().unwrap();
        state.messages += 1;
        state.messages
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
                    let _ = hub.handle(stream, port);
                });
            }
        });
        Ok(())
    }

    fn handle(&self, mut stream: TcpStream, port: u16) -> std::io::Result<()> {
        // a client that never finishes its request does not hold a thread for good
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request = String::new();
        reader.read_line(&mut request)?;
        let mut host = None;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                if name.trim().eq_ignore_ascii_case("host") {
                    host = Some(value.trim().to_string());
                }
            }
        }
        // A web page elsewhere can have its own name point at this machine
        // and so read the game's text through the browser; it gives that
        // name as the host.
        if !host.as_deref().is_some_and(|host| local(host, port)) {
            return stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
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

/// Whether a request's host names this machine, as the page's own address does.
fn local(host: &str, port: u16) -> bool {
    let name = host.strip_suffix(&format!(":{port}")).unwrap_or(host);
    ["127.0.0.1", "localhost"].iter().any(|own| name.eq_ignore_ascii_case(own))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_machine_is_served() {
        assert!(local("127.0.0.1:8777", 8777) && local("localhost:8777", 8777) && local("LOCALHOST", 80));
        assert!(!local("evil.example:8777", 8777) && !local("127.0.0.1.evil.example:8777", 8777));
        assert!(!local("localhost:8777", 9000));
    }
}
