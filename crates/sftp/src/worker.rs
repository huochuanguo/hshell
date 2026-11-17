use crate::{Connection, Entry, Progress, Transfer};
use ssh_core::{Profile, SessionEvent, SubsystemAuthentication};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

static WORKERS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());
struct ActiveWorker;
impl ActiveWorker {
    fn new() -> Self {
        *WORKERS.0.lock().unwrap() += 1;
        Self
    }
}
impl Drop for ActiveWorker {
    fn drop(&mut self) {
        *WORKERS.0.lock().unwrap() -= 1;
        WORKERS.1.notify_all();
    }
}
/// Call after stopping controls and dropping receivers, after the GUI loop.
/// This gives independent SSH children time to close and be reaped.
pub fn wait_for_shutdown(timeout: Duration) -> bool {
    let (count, _) = WORKERS
        .1
        .wait_timeout_while(WORKERS.0.lock().unwrap(), timeout, |n| *n != 0)
        .unwrap();
    *count == 0
}

pub struct WorkerControl {
    cancel: CancellationToken,
    auth: Arc<Mutex<Option<SubsystemAuthentication>>>,
}
impl WorkerControl {
    fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
            auth: Arc::default(),
        }
    }
    pub fn stop(&self) {
        self.cancel.cancel();
        self.auth.lock().unwrap().take();
    }
    pub fn respond(&self, id: &str, answer: Option<String>) -> Result<(), String> {
        self.auth
            .lock()
            .unwrap()
            .as_ref()
            .ok_or("SFTP 认证已结束")?
            .respond(id, answer)
    }
    pub fn authentication(&self) -> Option<SubsystemAuthentication> {
        self.auth.lock().unwrap().clone()
    }
}
impl Drop for WorkerControl {
    fn drop(&mut self) {
        self.stop();
    }
}

pub enum BrowseRequest {
    List { generation: u64, path: PathBuf },
    Mkdir(PathBuf),
    Rename(PathBuf, PathBuf),
    Remove(PathBuf, bool),
}
pub enum BrowserEvent {
    Ready,
    Entries(u64, Vec<Entry>),
    Listed(u64, Result<PathBuf, String>),
    Changed(Result<(), String>),
    Auth {
        id: String,
        prompt: String,
        confirm: bool,
    },
    Closed(String),
}

pub struct Browser {
    pub control: WorkerControl,
    sender: mpsc::Sender<BrowseRequest>,
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(2)
        .build()
        .map_err(|e| e.to_string())
}
fn password(
    profile: &Profile,
    path: &std::path::Path,
) -> Result<Option<Zeroizing<String>>, String> {
    let saved = ssh_core::load_profiles(path)?.into_iter().find(|saved| {
        saved.id == profile.id
            && saved.host == profile.host
            && saved.port == profile.port
            && saved.username == profile.username
    });
    saved
        .filter(|saved| saved.password_saved)
        .map(|saved| ssh_core::password_for(&ssh_core::SystemCredentials, &saved))
        .transpose()
}

impl Browser {
    pub fn start(
        profile: Profile,
        profiles_path: PathBuf,
        sink: impl Fn(BrowserEvent) -> bool + Send + Sync + 'static,
    ) -> Self {
        let control = WorkerControl::new();
        let cancel = control.cancel.clone();
        let auth_slot = control.auth.clone();
        let (sender, mut receiver) = mpsc::channel(4);
        let sink = Arc::new(sink);
        let active = ActiveWorker::new();
        std::thread::spawn(move || {
            let _active = active;
            let setup = (|| {
                let auth_sink = sink.clone();
                let (command, auth) = ssh_core::sftp_command(
                    &profile,
                    password(&profile, &profiles_path)?,
                    move |event| {
                        if let SessionEvent::Auth {
                            request_id,
                            prompt,
                            confirm,
                        } = event
                        {
                            auth_sink(BrowserEvent::Auth {
                                id: request_id,
                                prompt,
                                confirm,
                            })
                        } else {
                            false
                        }
                    },
                )?;
                *auth_slot.lock().unwrap() = Some(auth.clone());
                Ok::<_, String>((runtime()?, command, auth))
            })();
            match setup {
                Err(error) => { sink(BrowserEvent::Closed(error)); },
                Ok((runtime,command,auth)) => runtime.block_on(async {
                    let connection = tokio::select! { _=cancel.cancelled()=>return, result=Connection::open(command,auth)=>match result {Ok(connection)=>connection,Err(error)=>{sink(BrowserEvent::Closed(error));return;}} };
                    if !sink(BrowserEvent::Ready) { connection.close().await; return; }
                    loop {
                        let request = tokio::select! { _=cancel.cancelled()=>break, request=receiver.recv()=>match request {Some(request)=>request,None=>break} };
                        let operation = async {
                            match request {
                                BrowseRequest::List {generation,path} => {
                                    let result=connection.list(&path,|batch| if sink(BrowserEvent::Entries(generation,batch)) {Ok(())} else {Err("文件栏已关闭".into())}).await;
                                    sink(BrowserEvent::Listed(generation,result));
                                },
                                BrowseRequest::Mkdir(path)=>{sink(BrowserEvent::Changed(connection.mkdir(&path).await));},
                                BrowseRequest::Rename(from,to)=>{sink(BrowserEvent::Changed(connection.rename(&from,&to).await));},
                                BrowseRequest::Remove(path,directory)=>{sink(BrowserEvent::Changed(connection.remove(&path,directory).await));},
                            }
                        };
                        let timed_out=tokio::select! { _=cancel.cancelled()=>break, result=tokio::time::timeout(Duration::from_secs(30),operation)=>result.is_err() };
                        if timed_out {sink(BrowserEvent::Closed("文件操作超时，请重新连接；传输任务不受影响".into()));break;}
                    }
                    connection.close().await;
                }),
            }
            auth_slot.lock().unwrap().take();
        });
        Self { control, sender }
    }
    pub fn request(&self, request: BrowseRequest) -> Result<(), String> {
        self.sender
            .try_send(request)
            .map_err(|_| "文件操作仍在进行，请稍后重试".into())
    }
}

pub enum TransferEvent {
    Auth {
        id: String,
        prompt: String,
        confirm: bool,
    },
    Progress(Progress),
    Finished(Box<Transfer>, Result<(), String>),
}

pub fn start_transfer(
    mut transfer: Transfer,
    profiles_path: PathBuf,
    sink: impl Fn(TransferEvent) -> bool + Send + Sync + 'static,
) -> WorkerControl {
    let control = WorkerControl::new();
    let cancel = control.cancel.clone();
    let slot = control.auth.clone();
    let sink = Arc::new(sink);
    let active = ActiveWorker::new();
    std::thread::spawn(move || {
        let _active = active;
        let setup = (|| {
            let auth_sink = sink.clone();
            let (command, auth) = ssh_core::sftp_command(
                &transfer.profile,
                password(&transfer.profile, &profiles_path)?,
                move |event| {
                    if let SessionEvent::Auth {
                        request_id,
                        prompt,
                        confirm,
                    } = event
                    {
                        auth_sink(TransferEvent::Auth {
                            id: request_id,
                            prompt,
                            confirm,
                        })
                    } else {
                        false
                    }
                },
            )?;
            *slot.lock().unwrap() = Some(auth.clone());
            Ok::<_, String>((runtime()?, command, auth))
        })();
        let result=match setup {
            Err(error)=>Err(error),
            Ok((runtime,command,auth))=>runtime.block_on(async {
                let connection=tokio::select! {_=cancel.cancelled()=>return Err("已暂停，可继续传输".into()),result=Connection::open(command,auth)=>result?};
                let last_activity=Arc::new(Mutex::new(Instant::now()));let activity=last_activity.clone();
                let mut last_report=Instant::now()-Duration::from_secs(1);
                let transfer_future=transfer.run(&connection,|progress| {
                    *activity.lock().unwrap()=Instant::now();
                    if last_report.elapsed()>=Duration::from_millis(200) || progress.bytes==progress.total {
                        if !sink(TransferEvent::Progress(progress)) {cancel.cancel();}
                        last_report=Instant::now();
                    }
                });
                let watchdog=async {loop {tokio::time::sleep(Duration::from_secs(1)).await;if last_activity.lock().unwrap().elapsed()>Duration::from_secs(30) {break;}}};
                let result=tokio::select! {result=transfer_future=>result,_=cancel.cancelled()=>Err("已暂停，可继续传输".into()),_=watchdog=>Err("传输超过 30 秒无响应，已保留断点，请恢复传输".into())};
                connection.close().await;
                result
            }),
        };
        slot.lock().unwrap().take();
        sink(TransferEvent::Finished(Box::new(transfer), result));
    });
    control
}
