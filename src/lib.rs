//! context-footprint library — context graph construction and CF computation.

#[allow(clippy::doc_overindented_list_items)]
pub mod scip {
    include!(concat!(env!("OUT_DIR"), "/scip.rs"));
}

pub mod adapters;
pub mod app;
pub mod cli;
pub mod domain;
pub mod server;
