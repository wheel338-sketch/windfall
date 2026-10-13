//! Unix bridge GUI reports run on the control owner, never the audio callback.
use super::*;
use std::{cell::RefCell, time::Instant};
thread_local! { static POLLED: RefCell<BTreeMap<(usize, u64), Instant>> = const { RefCell::new(BTreeMap::new()) }; }
pub(super) fn maintain(
    owner: &Owner,
    errors: &Arc<Mutex<Vec<(PluginTarget, String)>>>,
    revision: &Arc<std::sync::atomic::AtomicU64>,
) {
    let now = Instant::now();
    let owner_key = Arc::as_ptr(&owner.selection) as usize;
    let due = POLLED.with(|polled| {
        let mut polled = polled.borrow_mut();
        polled.retain(|(key, token), _| *key != owner_key || owner.bridges.contains_key(token));
        owner
            .bridges
            .iter()
            .filter_map(|(token, record)| {
                if !record.playback
                    || record.revision != revision.load(std::sync::atomic::Ordering::Acquire)
                    || record.status().failed
                    || selected(&owner.selection, record.binding.target) != Some(*token)
                {
                    return None;
                }
                if polled
                    .get(&(owner_key, *token))
                    .is_some_and(|before| now.duration_since(*before) < Duration::from_secs(1))
                {
                    return None;
                }
                polled.insert((owner_key, *token), now);
                Some(*token)
            })
            .collect::<Vec<_>>()
    });
    for token in due {
        let record = &owner.bridges[&token];
        if let Ok((_, Some([application, monitor]))) = record.control.editor_report()
            && record.revision == revision.load(std::sync::atomic::Ordering::Acquire)
            && selected(&owner.selection, record.binding.target) == Some(token)
        {
            let mut errors = errors.lock().unwrap_or_else(|error| error.into_inner());
            errors.retain(|(target, _)| *target != record.binding.target);
            errors.push((record.binding.target, format!("The helper's plugin editor refused application scale {application} at monitor scale {monitor}.")));
        }
    }
}
