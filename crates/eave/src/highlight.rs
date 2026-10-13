//! Code, read into tokens so it can be colored (D-65).
//!
//! A small lexer over a table of what each language has: its keywords, its comments, its quotes.
//! It doesn't parse, so it's wrong now and then (a keyword used as a name), but it's a few KiB
//! where a real grammar engine would be megabytes, and code in the bar is read, not edited.

use std::ops::Range;

/// What a piece of code is, for its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Plain,
    Keyword,
    /// A type, a constant, a shell variable, a command's flag.
    Type,
    /// A function or macro being called, a command, a key in a table.
    Function,
    String,
    Number,
    Comment,
    Punctuation,
}

struct Syntax {
    keywords: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    /// Capitalized names are types.
    types: bool,
    /// A name or string followed by this is a key.
    key: Option<char>,
    /// The first word of a command is a function, `-x` is a flag, `$x` a variable.
    shell: bool,
}

const RUST: Syntax = Syntax {
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"'],
    types: true,
    key: None,
    shell: false,
};

const SHELL: Syntax = Syntax {
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case",
        "esac", "function", "return", "end", "and", "or", "not", "begin", "switch", "set",
        "export", "local",
    ],
    line_comments: &["#"],
    block_comment: None,
    quotes: &['"', '\''],
    types: false,
    key: None,
    shell: true,
};

const PYTHON: Syntax = Syntax {
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
        "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True",
        "try", "while", "with", "yield", "self",
    ],
    line_comments: &["#"],
    block_comment: None,
    quotes: &['"', '\''],
    types: true,
    key: None,
    shell: false,
};

const JS: Syntax = Syntax {
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "of",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "yield",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\'', '`'],
    types: true,
    key: None,
    shell: false,
};

const C: Syntax = Syntax {
    keywords: &[
        "auto",
        "break",
        "case",
        "char",
        "class",
        "const",
        "continue",
        "default",
        "delete",
        "do",
        "double",
        "else",
        "enum",
        "extern",
        "false",
        "float",
        "for",
        "if",
        "include",
        "inline",
        "int",
        "long",
        "namespace",
        "new",
        "nullptr",
        "private",
        "public",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "template",
        "this",
        "true",
        "typedef",
        "union",
        "unsigned",
        "using",
        "void",
        "volatile",
        "while",
        "define",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\''],
    types: true,
    key: None,
    shell: false,
};

const GO: Syntax = Syntax {
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "false",
        "fallthrough",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "true",
        "type",
        "var",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\'', '`'],
    types: true,
    key: None,
    shell: false,
};

const LUA: Syntax = Syntax {
    keywords: &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if",
        "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
    ],
    line_comments: &["--"],
    block_comment: None,
    quotes: &['"', '\''],
    types: false,
    key: Some('='),
    shell: false,
};

const TOML: Syntax = Syntax {
    keywords: &["true", "false"],
    line_comments: &["#"],
    block_comment: None,
    quotes: &['"', '\''],
    types: false,
    key: Some('='),
    shell: false,
};

const JSON: Syntax = Syntax {
    keywords: &["true", "false", "null"],
    line_comments: &[],
    block_comment: None,
    quotes: &['"'],
    types: false,
    key: Some(':'),
    shell: false,
};

const YAML: Syntax = Syntax {
    keywords: &["true", "false", "null", "yes", "no"],
    line_comments: &["#"],
    block_comment: None,
    quotes: &['"', '\''],
    types: false,
    key: Some(':'),
    shell: false,
};

/// For a language we don't know: strings, numbers and the usual comments.
const ANY: Syntax = Syntax {
    keywords: &[],
    line_comments: &["#", "//"],
    block_comment: None,
    quotes: &['"'],
    types: false,
    key: None,
    shell: false,
};

fn syntax(language: &str) -> &'static Syntax {
    match language.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => &RUST,
        "sh" | "bash" | "zsh" | "fish" | "shell" | "console" | "shellsession" => &SHELL,
        "python" | "py" => &PYTHON,
        "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "mjs" => &JS,
        "c" | "h" | "cpp" | "c++" | "cc" | "hpp" | "cxx" => &C,
        "go" => &GO,
        "lua" => &LUA,
        "toml" => &TOML,
        "json" | "jsonc" | "jsonl" => &JSON,
        "yaml" | "yml" => &YAML,
        _ => &ANY,
    }
}

/// `code`'s tokens, in order, covering all of it. Neighbors of the same kind are joined.
pub fn tokens(code: &str, language: &str) -> Vec<(Token, Range<usize>)> {
    let syntax = syntax(language);
    let rust = std::ptr::eq(syntax, &RUST);
    let bytes = code.as_bytes();
    let mut out: Vec<(Token, Range<usize>)> = Vec::new();
    let mut push = |token: Token, range: Range<usize>| {
        if range.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, previous)) if *last == token && previous.end == range.start => {
                previous.end = range.end;
            }
            _ => out.push((token, range)),
        }
    };
    // Where a shell command starts: at the start of a line, after `|`, `;`, `&&`, `(`.
    let mut command = true;
    let mut at = 0;
    while at < code.len() {
        let rest = &code[at..];
        let c = rest.chars().next().unwrap_or(' ');

        if let Some((open, close)) = syntax.block_comment
            && rest.starts_with(open)
        {
            let end = rest[open.len()..]
                .find(close)
                .map_or(code.len(), |i| at + open.len() + i + close.len());
            push(Token::Comment, at..end);
            at = end;
            continue;
        }
        // In a shell, `#` starts a comment only at the start of a word.
        let word_start = at == 0 || bytes[at - 1].is_ascii_whitespace();
        if syntax
            .line_comments
            .iter()
            .any(|start| rest.starts_with(start))
            && (!syntax.shell || word_start)
        {
            let end = rest.find('\n').map_or(code.len(), |i| at + i);
            push(Token::Comment, at..end);
            at = end;
            continue;
        }
        if syntax.quotes.contains(&c) || (rust && c == '\'' && is_char_literal(rest)) {
            let end = string_end(code, at, c);
            let token = if followed_by(code, end, syntax.key) {
                Token::Function
            } else {
                Token::String
            };
            push(token, at..end);
            at = end;
            command = false;
            continue;
        }
        if c.is_ascii_digit() {
            let end = scan(code, at, |c| {
                c.is_ascii_alphanumeric() || c == '_' || c == '.'
            });
            push(Token::Number, at..end);
            at = end;
            command = false;
            continue;
        }
        if syntax.shell && c == '$' {
            let end = scan(code, at + 1, |c| {
                c.is_alphanumeric() || c == '_' || c == '{' || c == '}'
            });
            push(Token::Type, at..end.max(at + 1));
            at = end.max(at + 1);
            continue;
        }
        if syntax.shell && c == '-' && word_start {
            let end = scan(code, at, |c| c.is_alphanumeric() || c == '-' || c == '_');
            push(Token::Type, at..end);
            at = end;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let end = if syntax.shell {
                scan(code, at, |c| {
                    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~')
                })
            } else {
                scan(code, at, |c| c.is_alphanumeric() || c == '_')
            };
            let word = &code[at..end];
            let next = code[end..].trim_start_matches([' ', '\t']).chars().next();
            let token = if syntax.keywords.contains(&word) {
                Token::Keyword
            } else if (syntax.shell && command)
                || followed_by(code, end, syntax.key)
                || (!syntax.shell && (next == Some('(') || (rust && next == Some('!'))))
            {
                Token::Function
            } else if syntax.types && word.starts_with(|c: char| c.is_uppercase()) {
                Token::Type
            } else {
                Token::Plain
            };
            // A macro's `!` goes with its name.
            let end = if rust && next == Some('!') && code[end..].starts_with('!') {
                end + 1
            } else {
                end
            };
            push(token, at..end);
            at = end;
            command = syntax.shell && matches!(word, "sudo" | "env" | "time" | "exec");
            continue;
        }
        let end = at + c.len_utf8();
        if c == '\n' || (syntax.shell && matches!(c, '|' | ';' | '&' | '(' | '`')) {
            command = true;
        }
        let token = if c.is_ascii_punctuation() {
            Token::Punctuation
        } else {
            Token::Plain
        };
        push(token, at..end);
        at = end;
    }
    out
}

/// Where the string starting with `quote` at `start` ends, past its closing quote. Strings in
/// double quotes and backticks may run over lines; others stop at the end of the line.
fn string_end(code: &str, start: usize, quote: char) -> usize {
    let multiline = quote == '`' || quote == '"';
    let mut escaped = false;
    for (i, c) in code[start + 1..].char_indices() {
        let at = start + 1 + i;
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return at + 1;
        } else if c == '\n' && !multiline {
            return at;
        }
    }
    code.len()
}

/// In Rust, `'x'` and `'\n'` are characters, while `'a` alone is a lifetime.
fn is_char_literal(rest: &str) -> bool {
    let mut chars = rest.chars().skip(1);
    match chars.next() {
        Some('\\') => true,
        Some(_) => chars.next() == Some('\''),
        None => false,
    }
}

fn scan(code: &str, from: usize, keep: impl Fn(char) -> bool) -> usize {
    code[from..]
        .char_indices()
        .find(|&(_, c)| !keep(c))
        .map_or(code.len(), |(i, _)| from + i)
}

fn followed_by(code: &str, at: usize, key: Option<char>) -> bool {
    let next = code[at..].trim_start_matches([' ', '\t']).chars().next();
    key.is_some() && next == key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(code: &str, language: &str) -> Vec<(Token, String)> {
        tokens(code, language)
            .into_iter()
            .filter(|(token, _)| *token != Token::Plain)
            .map(|(token, range)| (token, code[range].to_string()))
            .collect()
    }

    #[test]
    fn rust_reads_as_keywords_types_calls_and_comments() {
        let tokens = read(
            "fn on(&mut self) -> Task<M> { // hand\n    println!(\"hi {}\", 'a'); x.len() }",
            "rust",
        );
        let find = |text: &str| tokens.iter().find(|(_, t)| t == text).map(|(k, _)| *k);
        assert_eq!(find("fn"), Some(Token::Keyword));
        assert_eq!(find("mut"), Some(Token::Keyword));
        assert_eq!(find("on"), Some(Token::Function));
        assert_eq!(find("Task"), Some(Token::Type));
        assert_eq!(find("// hand"), Some(Token::Comment));
        assert_eq!(find("println!"), Some(Token::Function));
        assert_eq!(find("\"hi {}\""), Some(Token::String));
        assert_eq!(find("'a'"), Some(Token::String));
        assert_eq!(find("len"), Some(Token::Function));
    }

    #[test]
    fn a_lifetime_is_not_a_string() {
        let tokens = read("fn f<'a>(x: &'a str) {}", "rs");
        assert!(tokens.iter().all(|(token, _)| *token != Token::String));
    }

    #[test]
    fn shell_reads_commands_flags_and_variables() {
        let tokens = read(
            "cargo test --workspace # all\nls -la $HOME | grep 'x'",
            "sh",
        );
        assert_eq!(
            tokens,
            [
                (Token::Function, "cargo".into()),
                (Token::Type, "--workspace".into()),
                (Token::Comment, "# all".into()),
                (Token::Function, "ls".into()),
                (Token::Type, "-la".into()),
                (Token::Type, "$HOME".into()),
                (Token::Punctuation, "|".into()),
                (Token::Function, "grep".into()),
                (Token::String, "'x'".into()),
            ]
        );
    }

    #[test]
    fn keys_read_as_keys() {
        let tokens = read("[bar]\nname = \"eave\" # it\n", "toml");
        assert!(tokens.contains(&(Token::Function, "name".into())));
        assert!(tokens.contains(&(Token::String, "\"eave\"".into())));
        assert!(tokens.contains(&(Token::Comment, "# it".into())));
        let tokens = read("{\"a\": 1, \"b\": \"c\"}", "json");
        assert!(tokens.contains(&(Token::Function, "\"a\"".into())));
        assert!(tokens.contains(&(Token::String, "\"c\"".into())));
        assert!(tokens.contains(&(Token::Number, "1".into())));
    }

    #[test]
    fn tokens_cover_everything_even_unknown_and_broken_code() {
        for (code, language) in [
            ("x = \"unclosed", "python"),
            ("/* open", "c"),
            ("héllo wörld → 'ç'", "rust"),
            ("anything # at all", "brainfuck"),
            ("", "rust"),
        ] {
            let tokens = tokens(code, language);
            let mut at = 0;
            for (_, range) in &tokens {
                assert_eq!(range.start, at);
                at = range.end;
            }
            assert_eq!(at, code.len());
        }
    }
}
