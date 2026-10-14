//! Reads FL Studio project files and imports their musical structure.
//!
//! [`parse`] reads a bounded event stream into [`FlpProject`]. [`convert`]
//! dispatches the same [`windfall_project::Command`]s as the app and returns
//! a checked project, a plain-language report, and unsupported plugin states.
//! [`import`] composes both operations. Core paths do no file I/O and work
//! on `wasm32-unknown-unknown`; the inspection example owns its own file I/O.
//!
//! # Format sources and license
//!
//! This crate is GPL-3.0-or-later. Format knowledge was derived from:
//!
//! - [PyFLP](https://github.com/demberto/PyFLP), copyright 2022 demberto:
//!   GPL-3.0-or-later, explicitly granted by the Python file headers.
//!   Its package metadata says generic `GPL-3.0`; the headers resolve that
//!   ambiguity. Its LICENSE contains the GPL version 3 text.
//! - [DawVert](https://github.com/SatyrDiamond/DawVert), copyright 2024
//!   SatyrDiamond: GPL-3.0-or-later in the relevant Python SPDX headers.
//! - [FLParser](https://github.com/monadgroup/FLParser), copyright MONAD
//!   2017: its LICENSE explicitly grants GPL version 3 or any later version.
//! - [LMMS FLP importer at v1.0.3](https://github.com/LMMS/lmms/blob/v1.0.3/plugins/flp_import/FlpImport.cpp),
//!   copyright 2006-2014 Tobias Doerffel: GPL version 2 or any later version
//!   in the source header, compatible with this crate's GPL version 3 or later.
//!
//! See `NOTICE.md` and `docs/flp/coverage.md` for pinned revisions, evidence,
//! limitations and verification results. No proprietary implementation was
//! inspected. No Image-Line files, samples, presets or artwork are included.
//! Test streams are generated from original values by our CC0 fixture writer.
//!
//! # Confidence and limitations
//!
//! Container types, text, settings, channel controls, note records and modern
//! playlist records are supported by multiple open implementations. Historical
//! layouts and some plugin fields rely on one source; model comments identify
//! disagreements. Real-file verification currently covers FL Studio 20.8.4;
//! generated fixtures cover the historical and 21-era layouts, which does not
//! establish support for every release.
//!
//! Timing converts without rounding only when the source ticks land on the
//! 960 PPQ grid: 384 PPQ, for example, needs rounding for odd source ticks.
//! Unsupported plugins become silent instruments or retained effect states.
//! Native processor mappings approximate sound. Sample references remain
//! external, and unresolved folders are reported. Markers, additional
//! arrangements, recorded control events, unsupported automation targets and
//! sample stretching are reported as losses. The desktop shell stages this
//! conversion for review before replacing the current document.

pub mod error;
pub mod event;
pub mod model;
pub mod names;
pub mod parse;
pub mod plugin;
pub mod target;
mod text;

pub use error::{EventError, FlpError};
pub use event::{Container, Event, EventReader, EventValue, FileFormat, Header, open};
pub use model::*;
pub use parse::parse;
pub use plugin::{HostedPlugin, PluginFormat};
pub use target::{ChannelParam, ControlTarget, InsertParam, MainParam, SlotParam};

pub mod convert;
pub mod paths;
pub mod report;
pub mod units;
pub use convert::{Conversion, ConvertOptions, PluginPlace, PluginPlaceholder, convert, import};
pub use report::{ImportReport, OmittedControls, Outcome, ReportSection};
