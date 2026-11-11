//! `[paths]` config block and `Paths` resolution tests.
//!
//! Every test injects an explicit environment map and config dir, so no test
//! mutates process-global state and the module runs in parallel with no `HOME`
//! set. Each tree is pinned against one ladder: `NU_AGENT_<X>_HOME`, then
//! `[paths].<x>_home`, then `XDG_<X>_HOME`, then the XDG default under `HOME`.

// region:    --- Modules

mod config_block;
mod ladder;
mod rejection;
mod support;

// endregion: --- Modules
