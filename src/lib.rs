// src/lib.rs — Aven compiler core library
#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::should_implement_trait,
    clippy::unnecessary_map_or,
    clippy::too_many_arguments,
    clippy::chunks_exact_to_as_chunks,
    clippy::result_large_err
)]

pub mod api;
pub mod ast;
pub mod codegen_c;
pub mod codegen_wasm;
pub mod compaction;
pub mod debugger;
pub mod diagnostics;
pub mod diff;
pub mod evaluator;
pub mod ledger;
pub mod lexer;
pub mod optimize;
pub mod package;
pub mod parser;
pub mod smt;
pub mod solver;
pub mod temporal;
pub mod units;
