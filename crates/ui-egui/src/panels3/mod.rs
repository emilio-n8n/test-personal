//! The 3D modelling panels: the feature tree and the parameter panel.
//!
//! Both panels follow the same contract, and it is the contract the shell is built on:
//!
//! - They take **plain rows** — [`feature_tree::FeatureRow`], [`parameters::ParamRow`] — and know
//!   nothing about `cadcraft_model`. The model crate owns the features and their state; the shell
//!   reads them and hands over a row. That indirection is deliberate: a panel that imported the
//!   model would have to be rebuilt whenever the model grows a feature kind, and would drag the
//!   model crate into every test of the interface.
//! - They keep **no state of their own** beyond a struct the host owns
//!   ([`feature_tree::FeatureTree`], [`parameters::ParameterPanel`]) and egui's own memory.
//! - They **run no command**. Every edit leaves as an action value the host turns into an engine
//!   call, so the menu, the command line, a script and an automation client all reach the same code.
//!
//! The two rules both panels are built around are the ones a modelling tool most often breaks: a
//! **failed feature says so in the list**, with the message on the row rather than behind a hover,
//! and a **parameter that does not evaluate is still listed**, with the reason. A tree that hides
//! its own failures is a tree a user cannot debug.

pub mod feature_tree;
pub mod parameters;
