//! Bounded background SFTP browsing and durable, verified file resumption.
mod connection;
mod remote_path;
mod transfer;
mod worker;

pub use connection::{Connection, Entry};
pub use remote_path::{remote_absolute, remote_join, remote_name, remote_parent};
pub use transfer::{Direction, Progress, Transfer};
pub use worker::{
    start_transfer, wait_for_shutdown, BrowseRequest, Browser, BrowserEvent, TransferEvent,
    WorkerControl,
};
