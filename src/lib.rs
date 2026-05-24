pub mod compiler;
pub mod codegens;
pub mod generators;
pub mod editor;
pub mod ports;

pub mod project;
pub mod watch;
pub mod config;
pub mod wasm;
pub mod serve;

// Re-exports for internal convenience — avoids updating every `crate::ast` reference.
pub use compiler::token;
pub use compiler::lexer;
pub use compiler::ast;
pub use compiler::parser;
pub use compiler::error;
pub use compiler::link;
pub use compiler::verify;
pub use compiler::plan;
pub use editor::fmt;
pub use editor::lsp;
pub use editor::incremental;
pub use editor::diff;
pub use editor::constrain;
pub use generators::openapi;
pub use generators::typescript;
pub use generators::graphql;
pub use generators::deploy;
pub use generators::migrate;
pub use generators::testgen;
pub use generators::observability;
pub use ports::adapter;
