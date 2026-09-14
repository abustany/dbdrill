//! Runs database queries off the UI thread.
//!
//! `dbdrill-core` is blocking and single threaded, so a [`Db`] hands its
//! [`Session`] to a thread of its own and talks to it over channels. The UI
//! thread only ever polls for finished work, and so never blocks.
//!
//! Queries run one at a time, in the order they were requested.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use anyhow::Result;
use dbdrill_core::session::{QueryOutcome, Resources, Session};
use dbdrill_core::value::Row;

/// Identifies a query, so that the answer to a query nobody is waiting for any
/// more can be recognised and dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId(u64);

pub enum Request {
    Search {
        id: RequestId,
        resource_id: String,
        search_id: String,
        params: Vec<String>,
    },
    Link {
        id: RequestId,
        resource_id: String,
        link_name: String,
        row: Row,
    },
}

/// Something the worker finished doing.
pub enum Event {
    /// The database connection is up. Queries were queued until this point.
    Connected,
    /// The database connection could not be established. Nothing will run.
    ConnectionFailed(anyhow::Error),
    /// A query finished, successfully or not.
    Finished {
        id: RequestId,
        outcome: Result<QueryOutcome>,
    },
}

/// A handle on the thread running the queries.
pub struct Db {
    requests: Sender<Request>,
    events: Receiver<Event>,
    next_id: u64,
}

impl Db {
    /// Connects to `db_dsn` on a thread of its own and starts serving queries.
    ///
    /// Connecting happens on the worker too, so that a slow or unreachable
    /// database does not hold up the first frame. `repaint` is woken whenever
    /// an [`Event`] becomes available, so the UI does not have to poll on a
    /// timer.
    pub fn connect(
        db_dsn: String,
        resources: Arc<Resources>,
        repaint: impl Fn() + Send + 'static,
    ) -> Self {
        let (request_tx, request_rx) = channel();
        let (event_tx, event_rx) = channel();

        thread::Builder::new()
            .name("dbdrill-db".to_owned())
            .spawn(move || worker(db_dsn, resources, request_rx, event_tx, repaint))
            .expect("could not start the database thread");

        Db {
            requests: request_tx,
            events: event_rx,
            next_id: 0,
        }
    }

    /// Queues a search, and returns the id its [`Event::Finished`] will carry.
    pub fn search(&mut self, resource_id: &str, search_id: &str, params: Vec<String>) -> RequestId {
        let id = self.take_id();

        // A send only fails once the worker is gone, which happens when it
        // could not connect. The failure was already reported as an event.
        let _ = self.requests.send(Request::Search {
            id,
            resource_id: resource_id.to_owned(),
            search_id: search_id.to_owned(),
            params,
        });

        id
    }

    /// Queues a link to follow out of `row`, and returns the id its
    /// [`Event::Finished`] will carry.
    pub fn follow_link(&mut self, resource_id: &str, link_name: &str, row: Row) -> RequestId {
        let id = self.take_id();

        let _ = self.requests.send(Request::Link {
            id,
            resource_id: resource_id.to_owned(),
            link_name: link_name.to_owned(),
            row,
        });

        id
    }

    /// Returns whatever the worker finished since the last call, without ever
    /// waiting for it.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();

        loop {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return events,
            }
        }
    }

    /// A handle with no thread behind it, for tests that drive the events
    /// themselves: queries land in the returned receiver, and the returned
    /// sender stands in for the worker.
    #[cfg(test)]
    pub fn detached() -> (Self, Receiver<Request>, Sender<Event>) {
        let (request_tx, request_rx) = channel();
        let (event_tx, event_rx) = channel();

        let db = Db {
            requests: request_tx,
            events: event_rx,
            next_id: 0,
        };

        (db, request_rx, event_tx)
    }

    fn take_id(&mut self) -> RequestId {
        self.next_id += 1;
        RequestId(self.next_id)
    }
}

fn worker(
    db_dsn: String,
    resources: Arc<Resources>,
    requests: Receiver<Request>,
    events: Sender<Event>,
    repaint: impl Fn(),
) {
    let mut session = match Session::connect(&db_dsn, resources) {
        Ok(session) => {
            send(&events, &repaint, Event::Connected);
            session
        }
        Err(err) => {
            send(&events, &repaint, Event::ConnectionFailed(err));
            return;
        }
    };

    // Ends when the UI drops its handle, which is how the thread is stopped.
    while let Ok(request) = requests.recv() {
        let event = match request {
            Request::Search {
                id,
                resource_id,
                search_id,
                params,
            } => Event::Finished {
                id,
                outcome: session.run_search(&resource_id, &search_id, &params),
            },
            Request::Link {
                id,
                resource_id,
                link_name,
                row,
            } => Event::Finished {
                id,
                outcome: session.follow_link(&resource_id, &link_name, &row),
            },
        };

        send(&events, &repaint, event);
    }
}

fn send(events: &Sender<Event>, repaint: &impl Fn(), event: Event) {
    if events.send(event).is_ok() {
        repaint();
    }
}
