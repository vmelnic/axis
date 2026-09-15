pub mod codegens;
pub mod compiler;
pub mod editor;
pub mod generators;
pub mod ports;

pub mod config;
pub mod project;
pub mod serve;
pub mod wasm;
pub mod watch;

// Re-exports for internal convenience — avoids updating every `crate::ast` reference.
pub use compiler::ast;
pub use compiler::error;
pub use compiler::lexer;
pub use compiler::link;
pub use compiler::parser;
pub use compiler::plan;
pub use compiler::token;
pub use compiler::verify;
pub use editor::constrain;
pub use editor::diff;
pub use editor::fmt;
pub use editor::incremental;
pub use editor::lsp;
pub use generators::deploy;
pub use generators::graphql;
pub use generators::migrate;
pub use generators::observability;
pub use generators::openapi;
pub use generators::testgen;
pub use generators::typescript;
pub use ports::adapter;
