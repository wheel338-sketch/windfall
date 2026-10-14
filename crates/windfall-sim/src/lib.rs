//! The real project [`Document`] behind a small C ABI.
//!
//! The UI runs in two places: inside the app, against the shell, and in a
//! plain browser and its tests, against a mock backend. The mock runs this
//! crate compiled to WebAssembly, so both edit a project with the same code:
//! the same commands, limits, history labels, error messages and file format.
//! `scripts/build-sim.sh` builds the module and checks it in beside the mock.
//!
//! # Calls
//!
//! Every operation in [`ops`] is exported under its own name with one shape:
//!
//! ```text
//! name(handle: u32, input: *const u8, len: usize) -> *mut u8
//! ```
//!
//! `input` is UTF-8 JSON in a buffer from [`sim_alloc`], which the caller
//! frees again. An operation that takes no input ignores it. The result is a
//! new buffer that the caller frees with [`sim_dealloc`]: four bytes holding
//! the length of the text that follows, little-endian, then UTF-8 JSON that
//! is either `{"ok": value}` or `{"error": "message"}`. The messages are the
//! ones the app shows, since they come from the same error types.
//!
//! A document is named by the handle `doc_new` or `doc_from_file_json`
//! returned, until `doc_free`. Handles are never reused, so a stale one is
//! an error and never some other document.
//!
//! # Panics
//!
//! A panic means a bug in the document. Where panics unwind, the call
//! answers with an error and the document it was using is closed, because an
//! edit may have been cut off half way. In WebAssembly a panic aborts, the
//! caller sees a trap, and nothing in the module can be trusted afterwards.
//! [`sim_panic_message`] still answers then, with what the panic said.

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

use serde::{Deserialize, Serialize};
use windfall_project::Document;
mod files;
mod midi;
mod preset_files;
mod slicer;
mod timeline;

/// The JSON of the value a call produced, or the message of its failure.
pub type Reply = Result<String, String>;

/// One operation: a document handle and JSON in, a [`Reply`] out.
pub type Op = fn(u32, &str) -> Reply;

struct Documents {
    open: BTreeMap<u32, Document>,
    /// The handle given out last. Handles count up from 1.
    last: u32,
}

static DOCUMENTS: Mutex<Documents> = Mutex::new(Documents {
    open: BTreeMap::new(),
    last: 0,
});

/// What the last panic said and where, for [`sim_panic_message`].
static LAST_PANIC: Mutex<String> = Mutex::new(String::new());

// Neither lock is held while document code runs, only for a map lookup or a
// string copy. A poisoned lock therefore still guards sound data.
fn documents() -> MutexGuard<'static, Documents> {
    DOCUMENTS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn last_panic() -> MutexGuard<'static, String> {
    LAST_PANIC.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_open(handle: u32) -> String {
    windfall_core::notice!("sim.notOpen", handle = handle).to_string()
}

fn open(document: Document) -> Result<u32, String> {
    let mut documents = documents();
    let handle = documents
        .last
        .checked_add(1)
        .ok_or("the document handles have run out")?;
    documents.last = handle;
    documents.open.insert(handle, document);
    Ok(handle)
}

/// Runs `run` on an open document.
///
/// The document leaves the table while it is in use. A panic half way
/// through an edit therefore drops it, instead of leaving a document that
/// breaks its own rules open under a handle that still works.
fn with_document<T>(handle: u32, run: impl FnOnce(&mut Document) -> T) -> Result<T, String> {
    let mut document = documents()
        .open
        .remove(&handle)
        .ok_or_else(|| not_open(handle))?;
    let result = run(&mut document);
    documents().open.insert(handle, document);
    Ok(result)
}

fn parse<'a, T: Deserialize<'a>>(what: &str, input: &'a str) -> Result<T, String> {
    serde_json::from_str(input).map_err(|error| format!("invalid {what}: {error}"))
}

fn json(value: &impl Serialize) -> Reply {
    serde_json::to_string(value).map_err(|error| {
        windfall_core::notice!("sim.encodeFailed", error = error.to_string()).to_string()
    })
}

mod flp;

/// The operations, as plain functions over JSON text.
pub mod ops {
    /// The browser mock uses the native query grammar and tag normalization.
    pub fn browser_query(_handle: u32, input: &str) -> super::Reply {
        #[derive(serde::Deserialize)]
        struct Search {
            query: String,
            paths: Vec<String>,
        }
        let search: Search = super::parse("library search", input)?;
        if search.paths.len() > 50_000 || search.paths.iter().any(|p| p.len() > 4096) {
            return Err("Library search input exceeds its bounds.".into());
        }
        let query = windfall_ipc::LibraryQuery::parse(&search.query)?;
        super::json(
            &search
                .paths
                .iter()
                .enumerate()
                .filter_map(|(i, path)| query.matches(path).then_some(i))
                .collect::<Vec<_>>(),
        )
    }

    pub fn browser_tags(_handle: u32, input: &str) -> super::Reply {
        let tags: Vec<String> = super::parse("library tags", input)?;
        super::json(&windfall_ipc::normalize_tags(&tags)?)
    }
    pub use crate::files::{
        doc_score_capture, doc_score_command, doc_score_write, plugin_database_view, score_read,
    };
    pub use crate::midi::{doc_midi_export, doc_midi_import, midi_preview};
    pub use crate::preset_files::{
        doc_preset_capture, doc_preset_command, preset_entry, preset_parse,
    };
    pub use crate::slicer::{sample_playback_detect, slice_analyze, slice_command};
    pub use crate::timeline::{timeline_position, timeline_range};
    use serde::Deserialize;
    use windfall_project::file;
    use windfall_project::{Command, DispatchResult, Document, Project, SaveError, Touched};

    use super::{Reply, documents, json, not_open, open, parse, with_document};

    /// Reads and converts FL bytes for review without touching any document.
    pub fn flp_convert(handle: u32, input: &str) -> Reply {
        super::flp::convert(handle, input)
    }

    /// An empty project. Input: its name. Result: the `Project`.
    pub fn project_new(_handle: u32, input: &str) -> Reply {
        let name: String = parse("project name", input)?;
        json(&Project::new(name))
    }

    /// An imported project starts unsaved, so closing asks the user to save it.
    pub fn doc_new_unsaved(_handle: u32, input: &str) -> Reply {
        let project: Project = parse("project", input)?;
        project.check().map_err(|e| e.to_string())?;
        json(&open(Document::new_unsaved(project))?)
    }

    /// Opens a document on a project that is taken to be saved. Input: the
    /// `Project`, which must pass `Project::check`. Result: the new handle.
    pub fn doc_new(_handle: u32, input: &str) -> Reply {
        let project: Project = parse("project", input)?;
        project.check().map_err(|problem| {
            windfall_core::notice!("sim.invalidProject", problem = problem).to_string()
        })?;
        json(&open(Document::new(project))?)
    }

    /// Opens a document on the text of a `.windfall` file, with the checks
    /// and upgrades that loading a file has. Input: the text. Result: the
    /// new handle.
    pub fn doc_from_file_json(_handle: u32, input: &str) -> Reply {
        let text: String = parse("file text", input)?;
        let project = file::from_json(&text).map_err(|error| error.to_string())?;
        json(&open(Document::new(project))?)
    }

    /// Closes a document. Result: `null`.
    pub fn doc_free(handle: u32, _input: &str) -> Reply {
        let closed = documents().open.remove(&handle);
        closed.map(drop).ok_or_else(|| not_open(handle))?;
        json(&())
    }

    /// Applies a command and builds its patch, as the shell's `dispatch`
    /// does. Input: `{"command": Command, "gesture"?: number}`. Result: the
    /// `DispatchResult`.
    pub fn doc_dispatch(handle: u32, input: &str) -> Reply {
        #[derive(Deserialize)]
        struct Dispatch {
            command: Command,
            #[serde(default)]
            gesture: Option<u64>,
        }

        let Dispatch { command, gesture } = parse("command", input)?;
        with_document(handle, |document| {
            let applied = document
                .dispatch(command, gesture)
                .map_err(|error| error.to_string())?;
            json(&DispatchResult {
                created: applied.created,
                patch: document.patch(&applied.touched),
            })
        })?
    }

    pub fn doc_assistant(handle: u32, input: &str) -> Reply {
        use windfall_project::assistant::{
            AssistantContext, AssistantProvider, LOCAL_HELP, LocalRuleProvider,
        };
        #[derive(Deserialize)]
        struct Request {
            generation: u64,
            prompt: String,
        }
        let request: Request = parse("assistant request", input)?;
        with_document(handle, |document| {
            let stamp = windfall_project::proposal::ProposalStamp {
                generation: request.generation,
                revision: document.revision(),
            };
            let reply = LocalRuleProvider.respond(
                &request.prompt,
                AssistantContext {
                    project: document.project(),
                    stamp,
                    help: LOCAL_HELP,
                },
            )?;
            if let Some(proposal) = &reply.proposal {
                proposal.validate(document, stamp, false)?;
            }
            json(&reply)
        })?
    }

    pub fn doc_proposal_apply(handle: u32, input: &str) -> Reply {
        #[derive(Deserialize)]
        struct Request {
            generation: u64,
            recording: bool,
            proposal: windfall_project::proposal::CommandProposal,
        }
        let request: Request = parse("proposal", input)?;
        with_document(handle, |document| {
            let stamp = windfall_project::proposal::ProposalStamp {
                generation: request.generation,
                revision: document.revision(),
            };
            let applied = request.proposal.apply(document, stamp, request.recording)?;
            json(&DispatchResult {
                created: applied.created,
                patch: document.patch(&applied.touched),
            })
        })?
    }

    pub fn script_parameters(_handle: u32, input: &str) -> Reply {
        let source: String = parse("script source", input)?;
        let program =
            windfall_project::scripting::Program::compile(&source).map_err(|e| e.to_string())?;
        json(&program.parameters)
    }

    /// Undoes the last edit. Result: its `ProjectPatch`, or `null` when
    /// there was nothing to undo.
    pub fn doc_undo(handle: u32, _input: &str) -> Reply {
        with_document(handle, |document| {
            let touched = document.undo();
            json(&touched.map(|touched| document.patch(&touched)))
        })?
    }

    /// Applies the last undone edit again. Result: its `ProjectPatch`, or
    /// `null` when there was nothing to redo.
    pub fn doc_redo(handle: u32, _input: &str) -> Reply {
        with_document(handle, |document| {
            let touched = document.redo();
            json(&touched.map(|touched| document.patch(&touched)))
        })?
    }

    /// Undoes or redoes until that many history entries are applied. Input:
    /// the cursor. Result: the `ProjectPatch`.
    pub fn doc_jump(handle: u32, input: &str) -> Reply {
        let cursor: u32 = parse("history cursor", input)?;
        with_document(handle, |document| {
            let touched = document.jump(cursor);
            json(&document.patch(&touched))
        })?
    }

    /// Records that the project as it is now has been saved. Result: a
    /// `ProjectPatch` that changes nothing in the project and carries the
    /// new dirty flag, which is what the shell sends after a save.
    pub fn doc_mark_saved(handle: u32, _input: &str) -> Reply {
        with_document(handle, |document| {
            document.mark_saved();
            json(&document.patch(&Touched::default()))
        })?
    }

    /// The whole document. Input: the path of its file, or `null`. Result:
    /// the `DocumentSnapshot`.
    pub fn doc_snapshot(handle: u32, input: &str) -> Reply {
        let path: Option<String> = parse("path", input)?;
        with_document(handle, |document| json(&document.snapshot(path)))?
    }

    /// The id the next thing created will get. The shell reads it to refer,
    /// inside one batch, to a sample that the batch itself adds.
    pub fn doc_next_id(handle: u32, _input: &str) -> Reply {
        with_document(handle, |document| json(&document.project().next_id))?
    }

    /// The text of the `.windfall` file a save would write. A project that
    /// breaks a rule of the format is refused, as `file::save` refuses it.
    pub fn doc_to_file_json(handle: u32, _input: &str) -> Reply {
        with_document(handle, |document| json(&file_text(document)?))?
    }

    fn file_text(document: &Document) -> Result<String, String> {
        let project = document.project();
        project
            .check()
            .map_err(SaveError::Invalid)
            .and_then(|()| Ok(file::to_json(project)?))
            .map_err(|error| error.to_string())
    }

    /// Panics on purpose, with the input as the message, so that what a
    /// crash does can be tested from end to end. With a handle that is open,
    /// the panic happens while that document is in use.
    pub fn sim_panic(handle: u32, input: &str) -> Reply {
        let crash = || -> () { panic!("{input}") };
        if with_document(handle, |_| crash()).is_err() {
            crash();
        }
        json(&())
    }
}

/// Makes every panic leave its message and place in `LAST_PANIC`, and then
/// carry on to whatever handled panics before.
fn record_panics() {
    static HOOKED: Once = Once::new();
    HOOKED.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            *last_panic() = info.to_string();
            previous(info);
        }));
    });
}

/// Runs an operation and wraps what it gives in the `{"ok": …}` or
/// `{"error": …}` text a caller receives. A panic becomes an error.
pub fn call(op: Op, handle: u32, input: &str) -> String {
    record_panics();
    let reply = panic::catch_unwind(AssertUnwindSafe(|| op(handle, input))).unwrap_or_else(|_| {
        Err(windfall_core::notice!("sim.crashed", detail = last_panic().as_str()).to_string())
    });
    match reply {
        Ok(value) => format!("{{\"ok\":{value}}}"),
        Err(message) => {
            let message = serde_json::to_string(&message).unwrap_or_else(|_| "\"\"".to_owned());
            format!("{{\"error\":{message}}}")
        }
    }
}

/// Moves text into a buffer the caller owns: its length as four
/// little-endian bytes, then the text.
fn into_buffer(text: &str) -> *mut u8 {
    // A WebAssembly memory holds 4 GiB at most, so the length always fits.
    let len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    let mut buffer = Vec::with_capacity(4 + text.len());
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(text.as_bytes());
    Box::into_raw(buffer.into_boxed_slice()).cast()
}

/// Runs an operation on input that arrives as a pointer and a length.
///
/// # Safety
///
/// `input` must point to `len` bytes that can be read, unless `len` is 0.
unsafe fn respond(op: Op, handle: u32, input: *const u8, len: usize) -> *mut u8 {
    let bytes = if len == 0 {
        &[][..]
    } else {
        // SAFETY: the caller promises `len` readable bytes at `input`, and
        // the slice is not used after this call returns.
        unsafe { std::slice::from_raw_parts(input, len) }
    };
    let reply = match std::str::from_utf8(bytes) {
        Ok(text) => call(op, handle, text),
        Err(_) => call(|_, _| Err("the input is not UTF-8".to_owned()), handle, ""),
    };
    into_buffer(&reply)
}

/// A buffer of `len` zeroed bytes for the caller to fill and pass to an
/// operation. Free it with [`sim_dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn sim_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0_u8; len].into_boxed_slice()).cast()
}

/// Frees a buffer from [`sim_alloc`] or from an operation. A result is four
/// bytes longer than the text it holds.
///
/// # Safety
///
/// `buffer` must have come from this module with exactly this `len`, and
/// must not be used or freed again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sim_dealloc(buffer: *mut u8, len: usize) {
    let slice = std::ptr::slice_from_raw_parts_mut(buffer, len);
    // SAFETY: the caller promises the buffer is a boxed slice of `len` bytes
    // that this module handed out and nothing else still owns.
    drop(unsafe { Box::from_raw(slice) });
}

/// What the last panic said and where, as a result buffer holding plain
/// text. Empty when nothing has panicked. It runs no document code, so it
/// still answers after a panic has aborted another call.
#[unsafe(no_mangle)]
pub extern "C" fn sim_panic_message() -> *mut u8 {
    into_buffer(&last_panic())
}

macro_rules! export_ops {
    ($($name:ident)*) => {$(
        #[doc = concat!("[`ops::", stringify!($name), "`] for callers outside Rust.")]
        ///
        /// # Safety
        ///
        /// `input` must point to `len` bytes that can be read, unless `len`
        /// is 0.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: u32, input: *const u8, len: usize) -> *mut u8 {
            // SAFETY: the caller's promise is the one `respond` needs.
            unsafe { respond(ops::$name, handle, input, len) }
        }
    )*};
}

export_ops! {
    timeline_position
    timeline_range
    slice_analyze
    slice_command
    sample_playback_detect
    browser_query
    browser_tags
    project_new
    flp_convert
    doc_new
    doc_new_unsaved
    doc_from_file_json
    doc_free
    doc_dispatch
    doc_assistant
    doc_proposal_apply
    script_parameters
    doc_undo
    doc_redo
    doc_jump
    doc_mark_saved
    doc_snapshot
    doc_next_id
    doc_to_file_json
    sim_panic
    midi_preview
    doc_midi_import
    doc_midi_export
    doc_preset_capture
    doc_preset_command
    preset_parse
    preset_entry
    doc_score_capture
    doc_score_command
    doc_score_write
    score_read
    plugin_database_view
}

#[cfg(test)]
mod tests;
