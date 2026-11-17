//! Bounded background SFTP browsing and durable, verified file resumption.
mod connection;
mod transfer;
mod worker;

pub use connection::{Connection, Entry};
pub use transfer::{Direction, Progress, Transfer};
pub use worker::{
    start_transfer, wait_for_shutdown, BrowseRequest, Browser, BrowserEvent, TransferEvent,
    WorkerControl,
};
