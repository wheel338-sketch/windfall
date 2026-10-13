//! Cocoa owner pump. Native objects stay in a main-thread local map. A small
//! proxy thread delivers requests/ticks; it never constructs or drops an AU.
use super::*;
use std::{cell::RefCell, sync::OnceLock};
use windfall_plugin_host::gui::dispatch::Startup;
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
type Pump = Box<dyn FnMut(Option<Job>)>;
thread_local! { static OWNERS: RefCell<BTreeMap<u64, Pump>> = RefCell::new(BTreeMap::new()); }
unsafe extern "C" {
    fn pthread_main_np() -> i32;
}
fn is_main() -> bool {
    unsafe { pthread_main_np() != 0 }
}
pub(super) fn install(app: tauri::AppHandle) -> Result<(), String> {
    if !is_main() {
        return Err("Cocoa plugin dispatcher must be installed on the main thread".into());
    }
    APP.set(app)
        .map_err(|_| "Cocoa plugin dispatcher was already installed".into())
}
fn dispatch(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    if is_main() {
        job();
        return Ok(());
    }
    APP.get()
        .ok_or("Cocoa plugin dispatcher is not installed")?
        .run_on_main_thread(job)
        .map_err(|e| e.to_string())
}
pub(super) fn run_if_main(id: u64, job: Job) -> Result<Option<Job>, String> {
    if !is_main() {
        return Ok(Some(job));
    }
    OWNERS.with(|owners| {
        let mut owners = owners
            .try_borrow_mut()
            .map_err(|_| "Reentrant Cocoa plugin request")?;
        let owner = owners.get_mut(&id).ok_or("Cocoa plugin owner stopped")?;
        owner(Some(job));
        Ok(None)
    })
}
// These are the independently owned control queues/context, moved once to main.
#[allow(clippy::too_many_arguments)]
pub(super) fn start(
    receiver: mpsc::Receiver<Job>,
    selection: Selection,
    approved: Approved,
    controls: ControlRegistry,
    revision: Arc<std::sync::atomic::AtomicU64>,
    errors: Arc<Mutex<Vec<(PluginTarget, String)>>>,
    updates: mpsc::SyncSender<PendingUpdate>,
) -> Result<u64, String> {
    if APP.get().is_none() {
        return Err("Cocoa plugin dispatcher is not installed".into());
    }
    use std::sync::atomic::Ordering;
    let mut id = NEXT.load(Ordering::Relaxed);
    loop {
        let next = id
            .checked_add(1)
            .ok_or("Cocoa plugin owner identity exhausted")?;
        match NEXT.compare_exchange_weak(id, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => id = current,
        }
    }
    let (ready, initialized) = mpsc::sync_channel(1);
    let cancelled = Arc::new(Startup::default());
    let initialize_cancelled = cancelled.clone();
    dispatch(move || {
        if !initialize_cancelled.begin() {
            return;
        }
        let mut owner = Owner {
            selection,
            host: PluginHost::windfall(),
            instances: BTreeMap::new(),
            bridges: BTreeMap::new(),
            next: 0,
            approved,
            controls,
        };
        let mut gestures = std::collections::HashMap::new();
        let mut gesture = 1_u64 << 63;
        OWNERS.with(|owners| {
            owners.borrow_mut().insert(
                id,
                Box::new(move |job| {
                    if let Some(job) = job {
                        job(&mut owner);
                    }
                    maintain_owner(
                        &mut owner,
                        &revision,
                        &errors,
                        &updates,
                        &mut gestures,
                        &mut gesture,
                    );
                }),
            );
        });
        if !initialize_cancelled.publish()
            || ready.send(()).is_err()
            || initialize_cancelled.is_cancelled()
        {
            OWNERS.with(|owners| {
                owners.borrow_mut().remove(&id);
            });
        }
    })?;
    if initialized.recv_timeout(Duration::from_secs(15)).is_err() {
        cancelled.cancel();
        let _ = dispatch(move || {
            OWNERS.with(|owners| {
                owners.borrow_mut().remove(&id);
            });
        });
        return Err("Cocoa plugin initialization timed out".into());
    }
    let proxy = std::thread::Builder::new()
        .name("au-main-dispatch".into())
        .spawn(move || {
            loop {
                let job = match receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(job) => Some(job),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let (done, completed) = mpsc::sync_channel(1);
                if dispatch(move || {
                    OWNERS.with(|owners| {
                        if let Some(owner) = owners.borrow_mut().get_mut(&id) {
                            owner(job);
                        }
                    });
                    let _ = done.send(());
                })
                .is_err()
                {
                    break;
                }
                // Only this control proxy waits. There is at most one queued tick,
                // even while a modal editor stalls the app's event loop.
                if completed.recv().is_err() {
                    break;
                }
            }
            let _ = dispatch(move || {
                OWNERS.with(|owners| {
                    owners.borrow_mut().remove(&id);
                });
            });
        })
        .map_err(|e| e.to_string());
    if let Err(error) = proxy {
        cancelled.cancel();
        let _ = dispatch(move || {
            OWNERS.with(|owners| {
                owners.borrow_mut().remove(&id);
            });
        });
        return Err(error);
    }
    Ok(id)
}
