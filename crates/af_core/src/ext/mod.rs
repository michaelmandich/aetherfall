//! Per-crate extensions of the shared contract. Each file is owned by one
//! crate (named after it); other crates may read/use what it defines.

pub mod ai;
pub mod behavior;
pub mod combat;
pub mod economy;
pub mod fog;
pub mod input;
pub mod map;
pub mod movement;
pub mod net;
pub mod render;
pub mod ui;
