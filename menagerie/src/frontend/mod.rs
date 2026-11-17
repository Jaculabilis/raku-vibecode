//! Source-language frontends.

use crate::bytecode::{Lang, Program};

pub mod lox;
pub mod monkey;
mod monkey_macro;
pub mod tiger;

/// Guess a source file's language from its extension.
pub fn lang_for_path(path: &str) -> Option<Lang> {
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    match ext {
        "lox" => Some(Lang::Lox),
        "monkey" | "mky" | "mon" => Some(Lang::Monkey),
        "tig" | "tiger" => Some(Lang::Tiger),
        _ => None,
    }
}

pub fn lang_by_name(name: &str) -> Option<Lang> {
    match name {
        "lox" => Some(Lang::Lox),
        "monkey" => Some(Lang::Monkey),
        "tiger" => Some(Lang::Tiger),
        _ => None,
    }
}

/// Compile one source file.  On failure, returns the diagnostics, already
/// formatted the way the language's reference implementation formats them.
pub fn compile(lang: Lang, src: &str, module_name: &str) -> Result<Program, Vec<String>> {
    match lang {
        Lang::Lox => lox::compile(src, module_name),
        Lang::Monkey => monkey::compile(src, module_name),
        Lang::Tiger => tiger::compile(src, module_name),
    }
}
