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

pub mod ast;
pub mod codegen_c;
pub mod diagnostics;
pub mod diff;
pub mod evaluator;
pub mod ledger;
pub mod lexer;
pub mod parser;
pub mod solver;
pub mod units;
