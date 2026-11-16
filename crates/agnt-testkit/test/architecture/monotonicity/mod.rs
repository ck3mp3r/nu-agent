//! The ledger-direction drivers: a recorded baseline only ever shrinks.
//!
//! Every ledger row the tree records is compared to the rows the branch point
//! (`git merge-base main HEAD`) recorded. A key the branch point did not hold is
//! growth, and growth fails: the ratchet is shrink-only, and a row added in the
//! same commit as the surface it describes is the one path the exact-set check
//! cannot see.
//!
//! [`driver`] is the suite's one sanctioned git reader — the check itself takes
//! injected inputs and runs no process. It skips, with a printed reason, when git
//! is unavailable, when the checkout is shallow and when no `main` ref exists.
//!
//! The three growth exemptions are stated in the failure message: a ledger file
//! absent at the branch point, a branch that touches the walk's own sources, and
//! a key the same source file gave up in the same comparison. [`pure`] pins those
//! rules with injected inputs, so the exemption logic is testable without git.
//!
//! The ledger set is the per-crate files the census index declares plus the
//! `BASELINES` table. The index records no rows of its own, so it is never read
//! as a ledger; [`driver`] fails the read when a baseline file exists that the
//! index does not declare.

// region:    --- Modules

mod driver;
mod pure;

// endregion: --- Modules
