use crate::token::Span;
use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum AxisError {
    #[error("lexer error at line {line}, col {col}: {message}")]
    LexError {
        line: usize,
        col: usize,
        message: String,
    },

    #[error("parse error at line {line}, col {col}: {message}")]
    ParseError {
        line: usize,
        col: usize,
        message: String,
    },

    #[error("type error: {message}")]
    TypeError { span: Span, message: String },

    #[error("index error: {message}")]
    IndexError { span: Span, message: String },

    #[error("capability error: {message}")]
    CapabilityError { span: Span, message: String },

    #[error("policy violation: {message}")]
    PolicyViolation { span: Span, message: String },

    #[error("tenant error: {message}")]
    TenantError { span: Span, message: String },
}

impl AxisError {
    pub fn lex(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self::LexError {
            line,
            col,
            message: message.into(),
        }
    }

    pub fn parse(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self::ParseError {
            line,
            col,
            message: message.into(),
        }
    }
}

pub type AxisResult<T> = Result<T, AxisError>;
