// Fire-and-forget Redis publisher
//
// The matching thread hands finished messages to a channel and never waits on Redis I/O.
// One writer thread per Redis connection (response, settlement, risk) mirrors the three
// non-awaited clients in the TypeScript engine. Each writer drains whatever has queued up
// and sends it as one pipeline, like node-redis auto-pipelining, so a burst of messages
// costs one round trip and each message is still one LPUSH command.

use std::{
    sync::mpsc::{Receiver, Sender, channel},
    thread::{self, JoinHandle},
    time::Duration,
};

use redis::{Connection, pipe};

const MAX_BATCH: usize = 512;

pub struct Message {
    pub queue: String,
    pub payload: String,
}

pub struct Publisher {
    response: Sender<Message>,
    settlement: Sender<Message>,
    risk: Sender<Message>,
    _writers: Vec<JoinHandle<()>>,
}

impl Publisher {
    pub fn new(
        url: &str,
        response: Connection,
        settlement: Connection,
        risk: Connection,
        verbose: bool,
    ) -> Self {
        let (response_tx, response_handle) = spawn_writer("response", url, response, verbose);
        let (settlement_tx, settlement_handle) =
            spawn_writer("settlement", url, settlement, verbose);
        let (risk_tx, risk_handle) = spawn_writer("risk", url, risk, verbose);
        Self {
            response: response_tx,
            settlement: settlement_tx,
            risk: risk_tx,
            _writers: vec![response_handle, settlement_handle, risk_handle],
        }
    }

    pub fn response(&self, queue: String, payload: String) {
        let _ = self.response.send(Message { queue, payload });
    }

    pub fn settlement(&self, payload: String) {
        let _ = self.settlement.send(Message {
            queue: "settlement-queue".to_string(),
            payload,
        });
    }

    pub fn risk(&self, payload: String) {
        let _ = self.risk.send(Message {
            queue: "matching-to-risk-pub-queue".to_string(),
            payload,
        });
    }
}

fn spawn_writer(
    name: &'static str,
    url: &str,
    conn: Connection,
    verbose: bool,
) -> (Sender<Message>, JoinHandle<()>) {
    let (tx, rx) = channel::<Message>();
    let url = url.to_string();
    let handle = thread::Builder::new()
        .name(format!("redis-writer-{name}"))
        .spawn(move || writer_loop(name, url, conn, rx, verbose))
        .expect("failed to spawn redis writer");
    (tx, handle)
}

fn flush(conn: &mut Connection, batch: &[Message]) -> redis::RedisResult<()> {
    let mut p = pipe();
    for m in batch {
        p.cmd("LPUSH").arg(&m.queue).arg(&m.payload).ignore();
    }
    p.query::<()>(conn)
}

fn writer_loop(
    name: &'static str,
    url: String,
    mut conn: Connection,
    rx: Receiver<Message>,
    verbose: bool,
) {
    let mut batch: Vec<Message> = Vec::with_capacity(MAX_BATCH);
    while let Ok(first) = rx.recv() {
        batch.push(first);
        while batch.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(m) => batch.push(m),
                Err(_) => break,
            }
        }

        if let Err(e) = flush(&mut conn, &batch) {
            if verbose {
                println!("redis writer {name} error: {e}; reconnecting");
            }
            thread::sleep(Duration::from_millis(100));
            if let Ok(client) = redis::Client::open(url.as_str()) {
                if let Ok(c) = client.get_connection() {
                    conn = c;
                    let _ = flush(&mut conn, &batch);
                }
            }
        }
        batch.clear();
    }
}
