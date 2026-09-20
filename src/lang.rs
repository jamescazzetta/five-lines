//! The languages compiled into the binary, and the grammar node kinds each rule looks for.
//!
//! The rules are about structure, so one generic description covers every grammar:
//! what a function is, what an `if` is, what a switch is, what counts as a statement.

use tree_sitter::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Java,
    Go,
    Php,
    Rust,
    C,
    Cpp,
    CSharp,
}

impl Lang {
    pub fn from_path(path: &str) -> Option<Lang> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "py" => Lang::Python,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "java" => Lang::Java,
            "go" => Lang::Go,
            "php" => Lang::Php,
            "rs" => Lang::Rust,
            "c" | "h" => Lang::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" => Lang::Cpp,
            "cs" => Lang::CSharp,
            _ => return None,
        })
    }

    /// A name for extensions we cannot parse, so Jev still knows what it is reading.
    pub fn name_for_path(path: &str) -> String {
        match Lang::from_path(path) {
            Some(lang) => lang.name().to_string(),
            None => match path.rsplit('.').next().unwrap_or("") {
                "kt" | "kts" => "kotlin".into(),
                "swift" => "swift".into(),
                "rb" => "ruby".into(),
                "scala" => "scala".into(),
                "dart" => "dart".into(),
                _ => "unknown".into(),
            },
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript | Lang::Tsx => "typescript",
            Lang::Java => "java",
            Lang::Go => "go",
            Lang::Php => "php",
            Lang::Rust => "rust",
            Lang::C => "c",
            Lang::Cpp => "cpp",
            Lang::CSharp => "csharp",
        }
    }

    pub fn grammar(self) -> Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::C => tree_sitter_c::LANGUAGE.into(),
            Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        }
    }
}

pub const SUPPORTED: &str = "Python, JavaScript, TypeScript, Java, Go, PHP, Rust, C, C++, C#";

pub fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "method_declaration"
            | "constructor_declaration"
            | "func_literal"
            | "function_item"
            | "local_function_statement"
    )
}

pub fn is_if(kind: &str) -> bool {
    matches!(kind, "if_statement" | "if_expression")
}

/// Switch-like constructs in every supported grammar.
pub fn is_switch(kind: &str) -> bool {
    matches!(
        kind,
        "switch_statement"
            | "match_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "match_expression"
            | "switch_expression"
    )
}

pub fn is_arm(kind: &str) -> bool {
    matches!(
        kind,
        "case_clause"          // python
            | "switch_case"        // javascript / typescript
            | "switch_default"
            | "switch_block_statement_group" // java, classic switch
            | "switch_rule"        // java, arrow switch
            | "expression_case"    // go
            | "type_case"
            | "default_case"
            | "match_arm"          // rust
            | "case_statement"     // c / c++ / php
            | "default_statement"  // php
            | "switch_section"     // c#
            | "switch_expression_arm"
    )
}

/// A node that counts toward rule 1's budget.
pub fn is_statement(kind: &str) -> bool {
    let shaped = kind.ends_with("_statement") || kind.ends_with("_declaration") || kind == "declaration";
    shaped
        && !matches!(
            kind,
            // blocks and containers are not statements themselves
            "compound_statement" | "empty_statement" | "labeled_statement" | "case_statement" | "default_statement"
                // nested definitions are their own unit of review
                | "function_declaration" | "method_declaration" | "constructor_declaration"
                | "class_declaration" | "parameter_declaration" | "variadic_parameter_declaration"
                | "optional_parameter_declaration" | "field_declaration" | "local_function_statement"
        )
}

pub fn ends_the_arm(kind: &str) -> bool {
    matches!(kind, "return_statement" | "throw_statement" | "raise_statement" | "return_expression")
}
