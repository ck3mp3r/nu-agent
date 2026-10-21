//! Shared imports for the runtime picker test modules.
//!
//! Holds the imports and the helper re-exports every topical picker test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::runtime::render;
pub(super) use crate::runtime::status::help_test::{
    agent_picker_row_cells_for_test, model_picker_row_cells_for_test,
};
pub(super) use crate::runtime::test_driver::RenderLoopDriver;
pub(super) use crate::state::{ActivePicker, AppState};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
