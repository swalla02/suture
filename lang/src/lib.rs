//! The Suture language, prototype slice 1 (spec appendix B): statements,
//! controlled-English sentences, checks, read-backs and compiled programs.

pub mod ast;
pub mod lex;
pub mod sentence;

pub use lex::{statements, tokens, Cursor, Error, Stmt, Tok};
