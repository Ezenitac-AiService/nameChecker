//! Core domain logic for CJK cross-platform filename and folder inspection,
//! collation discrepancy detection, collision analysis, and safe renaming.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod collation;
pub mod collision;
pub mod detector;
pub mod encoding_hint;
pub mod i18n;
pub mod ignore_list;
pub mod model;
pub mod renamer;
pub mod report;
pub mod scanner;
pub mod unicode_data;
