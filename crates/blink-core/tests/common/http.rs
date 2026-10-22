//! A tiny scripted HTTP server for the feed tests.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Req {
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub enum Reply {
    /// Status line, extra headers, body.
    Http(u16, Vec<(&'static str, String)>, Vec<u8>),
    /// Declare a body but send nothing and close (for size checks).
    HeadersOnly(u16, Vec<(&'static str, String)>),
    /// Accept the connection and never answer.
    Hang,
}

pub struct TestServer {
    pub addr: SocketAddr,
    pub requests: Arc<Mutex<Vec<Req>>>,
}

impl TestServer {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub fn last(&self) -> Req {
        self.requests.lock().unwrap().last().cloned().expect("a request")
    }
}

/// `handler` sees each request (and its index) and decides the reply.
pub fn serve(handler: impl Fn(&Req, usize) -> Reply + Send + Sync + 'static) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let requests: Arc<Mutex<Vec<Req>>> = Arc::default();
    let log = requests.clone();
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (log, handler) = (log.clone(), handler.clone());
            thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                let mut lines = text.lines();
                let path = lines.next().unwrap_or("").split(' ').nth(1).unwrap_or("/").to_string();
                let headers = lines
                    .take_while(|l| !l.is_empty())
                    .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                    .collect();
                let req = Req { path, headers };
                let index = {
                    let mut l = log.lock().unwrap();
                    l.push(req.clone());
                    l.len() - 1
                };
                match handler(&req, index) {
                    Reply::Hang => thread::sleep(Duration::from_secs(5)),
                    Reply::HeadersOnly(status, extra) => {
                        let mut head = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
                        for (k, v) in extra {
                            head.push_str(&format!("{k}: {v}\r\n"));
                        }
                        head.push_str("\r\n");
                        let _ = stream.write_all(head.as_bytes());
                    }
                    Reply::Http(status, extra, body) => {
                        let mut head = format!("HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
                        for (k, v) in extra {
                            head.push_str(&format!("{k}: {v}\r\n"));
                        }
                        head.push_str("\r\n");
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(&body);
                    }
                }
            });
        }
    });
    TestServer { addr, requests }
}

pub fn ics(label: &str) -> Vec<u8> {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:{label}\r\nSUMMARY:{label} is due\r\nDTSTART:20260822T170000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
    .into_bytes()
}