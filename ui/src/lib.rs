//! KUI/GPUI frontend for KWork.
//!
//! Production builds target wasm32 and render inside the Tauri WebView; the
//! native host target only compiles this crate to run its unit tests, so
//! render-only items are legitimately unused there.
#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

#[cfg(any(target_family = "wasm", test))]
mod agent_terminals;
#[cfg(any(target_family = "wasm", test))]
pub mod anim;
#[cfg(any(target_family = "wasm", test))]
pub mod approval_command;
#[cfg(any(target_family = "wasm", test))]
mod assistant_stream;
#[cfg(any(target_family = "wasm", test))]
mod command_summary;
#[cfg(any(target_family = "wasm", test))]
mod commands;
pub mod data_url;
pub mod file_icons;
pub mod file_notes;
#[cfg(any(target_family = "wasm", test))]
mod files;
pub mod goal;
pub mod history;
pub mod layout;
#[cfg(any(target_family = "wasm", test))]
mod markdown;
pub mod media;
pub mod mentions;
pub mod process;
mod provider_marks;
mod questions;
#[cfg(any(target_family = "wasm", test))]
mod right_dock;
pub mod search_rules;
#[cfg(any(target_family = "wasm", test))]
mod session_order;
#[cfg(any(target_family = "wasm", test))]
mod session_reading;
#[cfg(any(target_family = "wasm", test))]
mod sessions;
#[cfg(any(target_family = "wasm", test))]
mod shortcuts;
pub mod state;
pub mod subagents;
pub mod submit;
#[path = "../../shared/submitted_plan.rs"]
mod submitted_plan;
#[cfg(any(target_family = "wasm", test))]
mod terminal_keys;
mod text_search;
pub mod theme_preference;
#[cfg(any(target_family = "wasm", test))]
pub mod tokens;
mod tool_calls;
pub mod tool_cards;
pub mod tool_detail;
pub mod tool_output;
pub mod tool_presentation;
#[cfg(any(target_family = "wasm", test))]
pub(crate) mod trajectory;
pub mod turns;
pub mod validation;
mod workflow_runs;
#[cfg(any(target_family = "wasm", test))]
mod workspace_tree;

#[cfg(target_family = "wasm")]
mod brand;
#[cfg(target_family = "wasm")]
mod bridge;
#[cfg(target_family = "wasm")]
mod web;
#[cfg(target_family = "wasm")]
mod welcome;

#[cfg(target_family = "wasm")]
pub use web::start;

#[cfg(not(target_family = "wasm"))]
/// Host-target placeholder; the executable frontend targets wasm32-unknown-unknown.
pub fn wasm_only() {}
