//! Every language the editor knows by its file, beyond the four it runs in a
//! console (R, Python, Julia, the shell - ide.rs `highlight`): what a file is
//! (`Kind`, from its name, else its first line), how it is coloured (`tokens`),
//! and what may be typed next (`complete`). Operator, 2026-11-01: "js, html,
//! css, scss, etc are supported for code coloring and completion as
//! available" - with JS, TS, JSX/TSX, HTML, CSS, SCSS/Sass, JSON, XML,
//! Markdown, SQL, YAML, PHP, C/C++/C#, Java named.
//!
//! One table, not a branch per language: a `Kind` names its `Family`, and a
//! family is one tokenizer (C-like, markup, stylesheet, YAML, key/value,
//! Markdown, SQL, line-directive). A new language is a row in `KINDS` and, if
//! it is like one already here, nothing else. Tokens come back as byte
//! ranges and a `Tok`; ide.rs turns them into colours. No egui here, so all
//! of it is tested natively.

/// What a stretch of a file is, for colouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Text,
    Keyword,
    /// true/false/null, a type name, a builtin.
    Constant,
    Number,
    String,
    Comment,
    Function,
    Operator,
    /// An HTML/XML tag, a CSS selector, a section header.
    Tag,
    /// An HTML attribute name.
    Attr,
    /// A CSS property, a YAML/TOML/JSON key.
    Property,
    /// $x, @x, ${x}.
    Variable,
    /// A Markdown heading.
    Heading,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    CLike,
    Markup,
    Style,
    Yaml,
    KeyValue,
    Markdown,
    Sql,
    /// A first word that is an instruction (Dockerfile, nginx, Makefile).
    Directive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    JavaScript,
    TypeScript,
    Jsx,
    Tsx,
    Json,
    Html,
    Xml,
    Css,
    Scss,
    Sass,
    Less,
    Markdown,
    Sql,
    Yaml,
    Toml,
    Php,
    C,
    Cpp,
    CSharp,
    Java,
    Go,
    Rust,
    Dockerfile,
    Nginx,
    Makefile,
}

/// (kind, name shown, completion name, family, extensions, exact file names)
struct Row {
    kind: Kind,
    label: &'static str,
    word: &'static str,
    family: Family,
    exts: &'static [&'static str],
    names: &'static [&'static str],
}

const KINDS: &[Row] = &[
    Row { kind: Kind::JavaScript, label: "JavaScript", word: "javascript", family: Family::CLike, exts: &["js", "mjs", "cjs"], names: &[] },
    Row { kind: Kind::TypeScript, label: "TypeScript", word: "typescript", family: Family::CLike, exts: &["ts", "mts", "cts"], names: &[] },
    Row { kind: Kind::Jsx, label: "JSX", word: "javascript", family: Family::CLike, exts: &["jsx"], names: &[] },
    Row { kind: Kind::Tsx, label: "TSX", word: "typescript", family: Family::CLike, exts: &["tsx"], names: &[] },
    Row { kind: Kind::Json, label: "JSON", word: "json", family: Family::CLike, exts: &["json", "jsonc", "webmanifest", "geojson"], names: &[".prettierrc", ".eslintrc"] },
    Row { kind: Kind::Html, label: "HTML", word: "html", family: Family::Markup, exts: &["html", "htm", "xhtml", "vue", "svelte"], names: &[] },
    Row { kind: Kind::Xml, label: "XML", word: "xml", family: Family::Markup, exts: &["xml", "svg", "xsd", "xsl", "plist", "rss", "atom"], names: &[] },
    Row { kind: Kind::Css, label: "CSS", word: "css", family: Family::Style, exts: &["css"], names: &[] },
    Row { kind: Kind::Scss, label: "SCSS", word: "scss", family: Family::Style, exts: &["scss"], names: &[] },
    Row { kind: Kind::Sass, label: "Sass", word: "scss", family: Family::Style, exts: &["sass"], names: &[] },
    Row { kind: Kind::Less, label: "Less", word: "css", family: Family::Style, exts: &["less"], names: &[] },
    Row { kind: Kind::Markdown, label: "Markdown", word: "markdown", family: Family::Markdown, exts: &["md", "markdown", "mdx"], names: &[] },
    Row { kind: Kind::Sql, label: "SQL", word: "sql", family: Family::Sql, exts: &["sql", "psql", "pgsql"], names: &[] },
    Row { kind: Kind::Yaml, label: "YAML", word: "yaml", family: Family::Yaml, exts: &["yml", "yaml"], names: &[] },
    Row { kind: Kind::Toml, label: "TOML", word: "toml", family: Family::KeyValue, exts: &["toml", "ini", "cfg", "conf", "env", "properties", "editorconfig"], names: &[".env", ".editorconfig", ".gitconfig"] },
    Row { kind: Kind::Php, label: "PHP", word: "php", family: Family::Markup, exts: &["php", "phtml"], names: &[] },
    Row { kind: Kind::C, label: "C", word: "c", family: Family::CLike, exts: &["c", "h"], names: &[] },
    Row { kind: Kind::Cpp, label: "C++", word: "cpp", family: Family::CLike, exts: &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "ino"], names: &[] },
    Row { kind: Kind::CSharp, label: "C#", word: "csharp", family: Family::CLike, exts: &["cs", "csx"], names: &[] },
    Row { kind: Kind::Java, label: "Java", word: "java", family: Family::CLike, exts: &["java", "kt", "kts", "gradle", "scala"], names: &[] },
    Row { kind: Kind::Go, label: "Go", word: "go", family: Family::CLike, exts: &["go"], names: &[] },
    Row { kind: Kind::Rust, label: "Rust", word: "rust", family: Family::CLike, exts: &["rs"], names: &[] },
    Row { kind: Kind::Dockerfile, label: "Dockerfile", word: "dockerfile", family: Family::Directive, exts: &["dockerfile", "containerfile"], names: &["Dockerfile", "Containerfile"] },
    Row { kind: Kind::Nginx, label: "nginx", word: "nginx", family: Family::Directive, exts: &[], names: &["nginx.conf"] },
    Row { kind: Kind::Makefile, label: "Makefile", word: "makefile", family: Family::Directive, exts: &["mk", "mak"], names: &["Makefile", "makefile", "GNUmakefile"] },
];

impl Kind {
    fn row(self) -> &'static Row {
        KINDS.iter().find(|r| r.kind == self).expect("every kind has a row")
    }
    pub fn label(self) -> &'static str {
        self.row().label
    }
    /// The name completion keys on ("javascript" for JS and JSX alike).
    pub fn word(self) -> &'static str {
        self.row().word
    }
    pub fn family(self) -> Family {
        self.row().family
    }
    /// HTML-like: tags close themselves and Emmet expands.
    pub fn markup(self) -> bool {
        matches!(self, Kind::Html | Kind::Xml | Kind::Php | Kind::Jsx | Kind::Tsx)
    }
    /// Two spaces by convention (web, data); four for the rest.
    pub fn indent_width(self) -> usize {
        match self {
            Kind::JavaScript | Kind::TypeScript | Kind::Jsx | Kind::Tsx | Kind::Json | Kind::Html | Kind::Xml | Kind::Css | Kind::Scss | Kind::Sass | Kind::Less | Kind::Yaml | Kind::Markdown => 2,
            _ => 4,
        }
    }
}

/// A file's kind from its name - an exact name first (Dockerfile, nginx.conf),
/// then its extension, then a `*.conf` beside nginx files.
pub fn of_path(path: &str) -> Option<Kind> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some(r) = KINDS.iter().find(|r| r.names.contains(&name)) {
        return Some(r.kind);
    }
    if (name.starts_with("Dockerfile.") || name.starts_with("Containerfile.")) && !name.ends_with(".md") {
        return Some(Kind::Dockerfile);
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    // nginx vhosts: sites-enabled/<host> has no extension; conf.d/*.conf does.
    if ext == "conf" && (path.contains("nginx") || path.contains("conf.d/")) {
        return Some(Kind::Nginx);
    }
    KINDS.iter().find(|r| r.exts.contains(&ext.as_str())).map(|r| r.kind)
}

/// A file with no telling name, by its first line.
pub fn of_text(text: &str) -> Option<Kind> {
    let first = text.trim_start().lines().next().unwrap_or("").trim();
    let low = first.to_ascii_lowercase();
    if low.starts_with("<!doctype html") || low.starts_with("<html") {
        Some(Kind::Html)
    } else if low.starts_with("<?xml") {
        Some(Kind::Xml)
    } else if low.starts_with("<?php") {
        Some(Kind::Php)
    } else if first.starts_with("#!") && first.contains("node") {
        Some(Kind::JavaScript)
    } else if (first.starts_with('{') || first.starts_with('[')) && text.trim_end().ends_with(['}', ']']) {
        Some(Kind::Json)
    } else if first.starts_with("---") || first.contains(": ") && !first.contains(';') && text.lines().take(5).all(|l| l.trim().is_empty() || l.contains(':') || l.trim_start().starts_with('-') || l.trim_start().starts_with('#')) {
        Some(Kind::Yaml)
    } else if text.contains("server {") && text.contains("listen ") {
        Some(Kind::Nginx)
    } else {
        None
    }
}

// ---- words -------------------------------------------------------------------------

const JS_KEYWORDS: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "export", "extends", "finally",
    "for", "from", "function", "if", "import", "in", "instanceof", "let", "new", "of", "return", "static", "super", "switch", "this",
    "throw", "try", "typeof", "var", "void", "while", "with", "yield", "async", "await", "get", "set",
];
const TS_KEYWORDS: &[&str] = &[
    "type", "interface", "enum", "implements", "namespace", "declare", "readonly", "private", "protected", "public", "abstract", "as",
    "keyof", "infer", "is", "satisfies", "module",
];
const JS_CONSTANTS: &[&str] = &["true", "false", "null", "undefined", "NaN", "Infinity"];
const TS_TYPES: &[&str] = &["string", "number", "boolean", "any", "unknown", "never", "void", "object", "bigint", "symbol", "Record", "Partial", "Readonly", "Promise", "Array"];
const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern", "float", "for", "goto",
    "if", "inline", "int", "long", "register", "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch",
    "typedef", "union", "unsigned", "void", "volatile", "while", "bool", "#include", "#define", "#ifdef", "#ifndef", "#endif", "#if", "#else", "#pragma",
];
const CPP_KEYWORDS: &[&str] = &[
    "class", "namespace", "template", "typename", "public", "private", "protected", "virtual", "override", "new", "delete", "this",
    "using", "try", "catch", "throw", "constexpr", "auto", "nullptr", "operator", "friend", "explicit", "noexcept", "static_cast",
    "dynamic_cast", "reinterpret_cast", "const_cast", "std",
];
const CS_KEYWORDS: &[&str] = &[
    "abstract", "as", "base", "bool", "break", "byte", "case", "catch", "char", "checked", "class", "const", "continue", "decimal",
    "default", "delegate", "do", "double", "else", "enum", "event", "explicit", "extern", "finally", "fixed", "float", "for",
    "foreach", "get", "goto", "if", "implicit", "in", "int", "interface", "internal", "is", "lock", "long", "namespace", "new",
    "object", "operator", "out", "override", "params", "private", "protected", "public", "readonly", "record", "ref", "return",
    "sealed", "set", "short", "sizeof", "static", "string", "struct", "switch", "this", "throw", "try", "typeof", "uint", "ulong",
    "using", "var", "virtual", "void", "while", "async", "await", "yield",
];
const JAVA_KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue", "default", "do", "double",
    "else", "enum", "extends", "final", "finally", "float", "for", "if", "implements", "import", "instanceof", "int", "interface",
    "long", "native", "new", "package", "private", "protected", "public", "return", "short", "static", "super", "switch",
    "synchronized", "this", "throw", "throws", "try", "void", "volatile", "while", "var", "record", "val", "fun", "when", "object",
];
const GO_KEYWORDS: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto", "if",
    "import", "interface", "map", "package", "range", "return", "select", "struct", "switch", "type", "var",
];
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
    "type", "unsafe", "use", "where", "while",
];
const PHP_KEYWORDS: &[&str] = &[
    "abstract", "and", "array", "as", "break", "case", "catch", "class", "clone", "const", "continue", "declare", "default", "do",
    "echo", "else", "elseif", "empty", "enddeclare", "endfor", "endforeach", "endif", "endswitch", "endwhile", "extends", "final",
    "finally", "fn", "for", "foreach", "function", "global", "if", "implements", "include", "include_once", "instanceof", "interface",
    "isset", "list", "match", "namespace", "new", "or", "print", "private", "protected", "public", "readonly", "require",
    "require_once", "return", "static", "switch", "throw", "trait", "try", "unset", "use", "var", "while", "yield",
];
const COMMON_CONSTANTS: &[&str] = &["true", "false", "null", "NULL", "nil", "None", "TRUE", "FALSE", "nullptr", "iota"];
const SQL_KEYWORDS: &[&str] = &[
    "select", "from", "where", "and", "or", "not", "insert", "into", "values", "update", "set", "delete", "create", "table", "view",
    "index", "drop", "alter", "add", "column", "primary", "key", "foreign", "references", "join", "left", "right", "inner", "outer",
    "full", "cross", "on", "as", "group", "by", "order", "having", "limit", "offset", "union", "all", "distinct", "case", "when",
    "then", "else", "end", "is", "null", "in", "between", "like", "ilike", "exists", "with", "returning", "default", "unique",
    "check", "constraint", "if", "begin", "commit", "rollback", "transaction", "grant", "revoke", "cascade", "asc", "desc",
    "true", "false", "coalesce", "count", "sum", "avg", "min", "max", "now", "cast", "over", "partition", "window", "lateral",
    "conflict", "do", "nothing", "returns", "function", "language", "trigger", "schema", "sequence", "serial", "bigserial",
    "integer", "bigint", "text", "varchar", "boolean", "timestamptz", "timestamp", "date", "jsonb", "json", "numeric", "uuid",
];
const DOCKER_WORDS: &[&str] = &["FROM", "RUN", "CMD", "LABEL", "EXPOSE", "ENV", "ADD", "COPY", "ENTRYPOINT", "VOLUME", "USER", "WORKDIR", "ARG", "ONBUILD", "STOPSIGNAL", "HEALTHCHECK", "SHELL", "AS"];

fn keywords(kind: Kind) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        Kind::JavaScript | Kind::Jsx => (JS_KEYWORDS, JS_CONSTANTS),
        Kind::TypeScript | Kind::Tsx => (JS_KEYWORDS, TS_TYPES),
        Kind::Json => (&[], JS_CONSTANTS),
        Kind::C => (C_KEYWORDS, COMMON_CONSTANTS),
        Kind::Cpp => (CPP_KEYWORDS, C_KEYWORDS),
        Kind::CSharp => (CS_KEYWORDS, COMMON_CONSTANTS),
        Kind::Java => (JAVA_KEYWORDS, COMMON_CONSTANTS),
        Kind::Go => (GO_KEYWORDS, COMMON_CONSTANTS),
        Kind::Rust => (RUST_KEYWORDS, COMMON_CONSTANTS),
        Kind::Php => (PHP_KEYWORDS, COMMON_CONSTANTS),
        _ => (&[], COMMON_CONSTANTS),
    }
}

// ---- tokens -------------------------------------------------------------------------

/// Byte ranges and what each is, covering `src` end to end. Anything not
/// recognised is Text, so a file never loses characters to the colouring.
pub fn tokens(src: &str, kind: Kind) -> Vec<(usize, usize, Tok)> {
    let mut out = Vec::new();
    match kind.family() {
        Family::CLike => clike(src, 0, src.len(), kind, &mut out),
        Family::Markup => markup(src, kind, &mut out),
        Family::Style => style(src, 0, src.len(), kind, &mut out),
        Family::Yaml => yaml(src, &mut out),
        Family::KeyValue => keyvalue(src, &mut out),
        Family::Markdown => markdown(src, &mut out),
        Family::Sql => sql(src, 0, src.len(), &mut out),
        Family::Directive => directive(src, kind, &mut out),
    }
    fill(src.len(), out)
}

/// Gaps between tokens become Text; overlaps are dropped.
fn fill(len: usize, mut toks: Vec<(usize, usize, Tok)>) -> Vec<(usize, usize, Tok)> {
    toks.sort_by_key(|t| t.0);
    let mut out = Vec::with_capacity(toks.len() * 2);
    let mut at = 0;
    for (a, b, t) in toks {
        if a < at || b <= a {
            continue;
        }
        if a > at {
            out.push((at, a, Tok::Text));
        }
        out.push((a, b, t));
        at = b;
    }
    if at < len {
        out.push((at, len, Tok::Text));
    }
    out
}

fn char_len(src: &str, i: usize) -> usize {
    src[i..].chars().next().map_or(1, char::len_utf8)
}

/// A quoted string from `i` (the quote), to the closing quote or the line's end.
fn string_end(b: &[u8], i: usize, multiline: bool) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            b'\n' if !multiline => return j,
            _ => j += 1,
        }
    }
    b.len()
}

fn number_end(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'.' || b[j] == b'_') {
        j += 1;
    }
    j
}

fn ident_end(b: &[u8], i: usize, extra: &[u8]) -> usize {
    let mut j = i;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || extra.contains(&b[j]) || b[j] >= 0x80) {
        j += 1;
    }
    j
}

fn clike(src: &str, from: usize, to: usize, kind: Kind, out: &mut Vec<(usize, usize, Tok)>) {
    let b = src.as_bytes();
    let (kw, konst) = keywords(kind);
    let ts = matches!(kind, Kind::TypeScript | Kind::Tsx);
    let jsx = matches!(kind, Kind::Jsx | Kind::Tsx);
    let hash_comment = kind == Kind::Php;
    let mut i = from;
    // The last thing that was not a space: decides whether `<` opens JSX and
    // whether `/` starts a regex.
    let mut prev: u8 = b'(';
    while i < to {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') || hash_comment && c == b'#' && b.get(i + 1) != Some(&b'[') {
            let e = src[i..to].find('\n').map_or(to, |n| i + n);
            out.push((i, e, Tok::Comment));
            i = e;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let e = src[i + 2..to].find("*/").map_or(to, |n| i + 2 + n + 2);
            out.push((i, e, Tok::Comment));
            i = e;
            continue;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            let e = string_end(b, i, c == b'`').min(to);
            // JSON: a string followed by ':' is a key.
            let tok = if kind == Kind::Json && src[e..to].trim_start().starts_with(':') { Tok::Property } else { Tok::String };
            out.push((i, e, tok));
            i = e;
            prev = b'a';
            continue;
        }
        if jsx && c == b'<' && (b.get(i + 1) == Some(&b'/') || prev == b'(' || prev == b'=' || prev == b'>' || prev == b'{' || prev == b',' || prev == b'?' || prev == b':' || prev == b'&' || prev == b'|' || prev == b'n')
            && b.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic() || *n == b'/' || *n == b'>')
        {
            // A JSX element: tags and attributes as in HTML, to its '>'.
            let e = tag_end(b, i, to);
            tag_tokens(src, i, e, out);
            i = e;
            prev = b'>';
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let e = number_end(b, i).min(to);
            out.push((i, e, Tok::Number));
            i = e;
            prev = b'0';
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c == b'#' && matches!(kind, Kind::C | Kind::Cpp) {
            let e = ident_end(b, i + 1, b"$").min(to);
            let w = &src[i..e];
            let next = src[e..to].trim_start_matches([' ', '\t']);
            let tok = if kw.contains(&w) {
                Tok::Keyword
            } else if konst.contains(&w) || JS_CONSTANTS.contains(&w) && matches!(kind, Kind::JavaScript | Kind::Jsx | Kind::TypeScript | Kind::Tsx | Kind::Json) {
                Tok::Constant
            } else if next.starts_with('(') || kind == Kind::Rust && next.starts_with('!') {
                Tok::Function
            } else if kind == Kind::Php && c == b'$' {
                Tok::Variable
            } else if (ts || matches!(kind, Kind::Java | Kind::CSharp | Kind::Rust | Kind::Go | Kind::Cpp)) && c.is_ascii_uppercase() {
                Tok::Constant
            } else {
                Tok::Text
            };
            out.push((i, e, tok));
            i = e;
            prev = if tok == Tok::Keyword && matches!(w, "return" | "in" | "of" | "typeof") { b'n' } else { b'a' };
            continue;
        }
        if b"+-*/%=<>!&|^~?:".contains(&c) {
            out.push((i, i + 1, Tok::Operator));
        }
        if !c.is_ascii_whitespace() {
            prev = c;
        }
        i += char_len(src, i);
    }
}

/// From a `<` to the `>` that ends the tag, quotes respected.
fn tag_end(b: &[u8], i: usize, to: usize) -> usize {
    let mut j = i + 1;
    while j < to {
        match b[j] {
            b'"' | b'\'' => j = string_end(b, j, true),
            b'{' => {
                // a JSX expression in an attribute
                let mut depth = 0;
                while j < to {
                    match b[j] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                j += 1;
            }
            b'>' => return j + 1,
            _ => j += 1,
        }
    }
    to
}

/// One tag: `<`, its name, attribute names and values, `>`.
fn tag_tokens(src: &str, i: usize, e: usize, out: &mut Vec<(usize, usize, Tok)>) {
    let b = src.as_bytes();
    let mut j = i + 1;
    if b.get(j) == Some(&b'/') {
        j += 1;
    }
    let name_end = ident_end(b, j, b"-:.").min(e);
    out.push((i, name_end, Tok::Tag));
    j = name_end;
    while j < e {
        let c = b[j];
        if c == b'"' || c == b'\'' {
            let s = string_end(b, j, true).min(e);
            out.push((j, s, Tok::String));
            j = s;
        } else if c.is_ascii_alphabetic() || c == b'@' || c == b':' || c == b'_' {
            let s = ident_end(b, j + 1, b"-:.@").min(e);
            out.push((j, s, Tok::Attr));
            j = s;
        } else if c == b'/' || c == b'>' {
            out.push((j, (j + 1).min(e), Tok::Tag));
            j += 1;
        } else {
            j += char_len(src, j);
        }
    }
}

fn markup(src: &str, kind: Kind, out: &mut Vec<(usize, usize, Tok)>) {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if src[i..].starts_with("<!--") {
            let e = src[i + 4..].find("-->").map_or(b.len(), |n| i + 4 + n + 3);
            out.push((i, e, Tok::Comment));
            i = e;
        } else if src[i..].starts_with("<?php") || (kind == Kind::Php && src[i..].starts_with("<?=")) {
            let open = if src[i..].starts_with("<?php") { 5 } else { 3 };
            out.push((i, i + open, Tok::Keyword));
            let e = src[i + open..].find("?>").map_or(b.len(), |n| i + open + n);
            clike(src, i + open, e, Kind::Php, out);
            if e + 2 <= b.len() {
                out.push((e, e + 2, Tok::Keyword));
            }
            i = (e + 2).min(b.len());
        } else if src[i..].starts_with("<![CDATA[") {
            let e = src[i..].find("]]>").map_or(b.len(), |n| i + n + 3);
            out.push((i, e, Tok::String));
            i = e;
        } else if b[i] == b'<' && b.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic() || *n == b'/' || *n == b'!' || *n == b'?') {
            let e = tag_end(b, i, b.len());
            tag_tokens(src, i, e, out);
            // <script> and <style> bodies in their own languages.
            let name: String = src[i + 1..e].chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            if (name == "script" || name == "style") && !src[i..e].ends_with("/>") {
                let close = format!("</{name}");
                let body_end = src[e..].to_ascii_lowercase().find(&close).map_or(b.len(), |n| e + n);
                if name == "script" {
                    clike(src, e, body_end, Kind::JavaScript, out);
                } else {
                    style(src, e, body_end, Kind::Css, out);
                }
                i = body_end;
            } else {
                i = e;
            }
        } else if b[i] == b'&' {
            let e = src[i..].find(';').filter(|n| *n < 10).map_or(i + 1, |n| i + n + 1);
            out.push((i, e, Tok::Constant));
            i = e;
        } else {
            i += char_len(src, i);
        }
    }
}

fn style(src: &str, from: usize, to: usize, kind: Kind, out: &mut Vec<(usize, usize, Tok)>) {
    let b = src.as_bytes();
    let line_comments = matches!(kind, Kind::Scss | Kind::Sass | Kind::Less);
    let mut depth = 0i32;
    let mut i = from;
    // Inside a block, a word before ':' on its statement is a property; after
    // it, a value - until ';' or '{' or '}'.
    let mut in_value = false;
    while i < to {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let e = src[i + 2..to].find("*/").map_or(to, |n| i + 2 + n + 2);
            out.push((i, e, Tok::Comment));
            i = e;
            continue;
        }
        if line_comments && c == b'/' && b.get(i + 1) == Some(&b'/') {
            let e = src[i..to].find('\n').map_or(to, |n| i + n);
            out.push((i, e, Tok::Comment));
            i = e;
            continue;
        }
        match c {
            b'{' => {
                depth += 1;
                in_value = false;
            }
            b'}' => {
                depth -= 1;
                in_value = false;
            }
            b';' => in_value = false,
            b'\n' if kind == Kind::Sass => in_value = false,
            _ => {}
        }
        if c == b'"' || c == b'\'' {
            let e = string_end(b, i, false).min(to);
            out.push((i, e, Tok::String));
            i = e;
            continue;
        }
        if c == b'$' || (kind == Kind::Less && c == b'@' && in_value) {
            let e = ident_end(b, i + 1, b"-").min(to);
            out.push((i, e, Tok::Variable));
            i = e;
            continue;
        }
        if c == b'@' {
            let e = ident_end(b, i + 1, b"-").min(to);
            out.push((i, e, Tok::Keyword));
            i = e;
            continue;
        }
        if c == b'#' && in_value {
            let e = ident_end(b, i + 1, b"").min(to);
            out.push((i, e, Tok::Number));
            i = e;
            continue;
        }
        if in_value && (c.is_ascii_digit() || (c == b'.' || c == b'-') && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let mut e = i + 1;
            while e < to && (b[e].is_ascii_alphanumeric() || b[e] == b'.' || b[e] == b'%') {
                e += 1;
            }
            out.push((i, e, Tok::Number));
            i = e;
            continue;
        }
        if c == b'!' && in_value {
            let e = ident_end(b, i + 1, b"").min(to);
            out.push((i, e, Tok::Keyword));
            i = e;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'-' || c == b'_' || ((c == b'.' || c == b'#' || c == b'&' || c == b':') && !in_value) {
            let e = ident_end(b, i + 1, b"-").min(to);
            let next = src[e..to].trim_start_matches([' ', '\t']);
            let tok = if in_value {
                if next.starts_with('(') { Tok::Function } else { Tok::Constant }
            } else if depth > 0 && next.starts_with(':') && !next.starts_with("::") && !looks_like_selector(next) {
                in_value = true;
                Tok::Property
            } else {
                Tok::Tag
            };
            out.push((i, e, tok));
            i = e;
            continue;
        }
        if c == b':' && depth > 0 && !in_value {
            in_value = true;
        }
        i += char_len(src, i);
    }
}

/// After `a:`, does the line go on like a selector (`a:hover {`) rather than a
/// declaration (`color: red;`)?
fn looks_like_selector(next: &str) -> bool {
    let line = next.split(['\n', ';']).next().unwrap_or("");
    line.trim_end().ends_with('{') || line.trim_end().ends_with(',')
}

fn yaml(src: &str, out: &mut Vec<(usize, usize, Tok)>) {
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let ind = line.len() - t.len();
        if t.starts_with('#') {
            out.push((at + ind, at + line.trim_end().len(), Tok::Comment));
        } else if t.starts_with("---") || t.starts_with("...") {
            out.push((at + ind, at + ind + 3, Tok::Keyword));
        } else {
            let body = t.strip_prefix("- ").map_or(ind, |_| ind + 2);
            if body > ind {
                out.push((at + ind, at + ind + 1, Tok::Operator));
            }
            // key: value
            let rest = &line[body..];
            let key_len = if rest.starts_with(['"', '\'']) { string_end(rest.as_bytes(), 0, false) } else { rest.find(": ").or_else(|| rest.trim_end().strip_suffix(':').map(|k| k.len())).unwrap_or(0) };
            let mut v = body;
            if key_len > 0 && rest[key_len..].starts_with(':') && !rest[..key_len].contains(' ') || key_len > 0 && rest.starts_with(['"', '\'']) {
                out.push((at + body, at + body + key_len, Tok::Property));
                v = body + key_len + 1;
            }
            // the value
            let val = line[v.min(line.len())..].trim();
            if !val.is_empty() {
                let vs = at + line[..].find(val).map_or(v, |p| p.max(v));
                let comment = val.find(" #");
                let shown = comment.map_or(val, |c| &val[..c]);
                let tok = if shown.starts_with(['"', '\'']) {
                    Tok::String
                } else if shown.parse::<f64>().is_ok() {
                    Tok::Number
                } else if matches!(shown, "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off") {
                    Tok::Constant
                } else if shown.starts_with(['&', '*', '!']) {
                    Tok::Variable
                } else if shown.starts_with(['|', '>']) {
                    Tok::Operator
                } else {
                    Tok::String
                };
                out.push((vs, vs + shown.len(), tok));
                if let Some(c) = comment {
                    out.push((vs + c + 1, vs + val.len(), Tok::Comment));
                }
            }
        }
        at += line.len();
    }
}

fn keyvalue(src: &str, out: &mut Vec<(usize, usize, Tok)>) {
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let ind = line.len() - t.len();
        let end = line.trim_end().len();
        if t.starts_with('#') || t.starts_with(';') {
            out.push((at + ind, at + end, Tok::Comment));
        } else if t.starts_with('[') {
            out.push((at + ind, at + end, Tok::Tag));
        } else if let Some(eq) = line.find('=') {
            out.push((at + ind, at + line[..eq].trim_end().len(), Tok::Property));
            let val = line[eq + 1..].trim();
            if !val.is_empty() {
                let vs = at + eq + 1 + line[eq + 1..].find(val).unwrap_or(0);
                let tok = if val.starts_with(['"', '\'']) {
                    Tok::String
                } else if val.parse::<f64>().is_ok() {
                    Tok::Number
                } else if matches!(val, "true" | "false") {
                    Tok::Constant
                } else {
                    Tok::String
                };
                out.push((vs, vs + val.len(), tok));
            }
        }
        at += line.len();
    }
}

fn markdown(src: &str, out: &mut Vec<(usize, usize, Tok)>) {
    let mut at = 0;
    let mut fence: Option<(String, Option<Kind>)> = None;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let end = at + line.trim_end().len();
        if let Some((marker, inner)) = &fence {
            if t.starts_with(marker.as_str()) {
                out.push((at, end, Tok::Comment));
                fence = None;
            } else if let Some(k) = inner {
                let mut sub = tokens(line, *k);
                for s in sub.iter_mut() {
                    s.0 += at;
                    s.1 += at;
                }
                out.extend(sub.into_iter().filter(|s| s.2 != Tok::Text));
            } else {
                out.push((at, end, Tok::String));
            }
            at += line.len();
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            let marker = t[..3].to_owned();
            let info = t[3..].trim().trim_matches(['{', '}']).split([' ', ',']).next().unwrap_or("").to_ascii_lowercase();
            let inner = of_path(&format!("x.{info}")).or(match info.as_str() {
                "javascript" | "node" => Some(Kind::JavaScript),
                "typescript" => Some(Kind::TypeScript),
                "rust" => Some(Kind::Rust),
                "csharp" | "c#" => Some(Kind::CSharp),
                "c++" => Some(Kind::Cpp),
                "golang" => Some(Kind::Go),
                "docker" | "dockerfile" => Some(Kind::Dockerfile),
                "nginx" => Some(Kind::Nginx),
                "make" | "makefile" => Some(Kind::Makefile),
                _ => None,
            });
            out.push((at, end, Tok::Comment));
            fence = Some((marker, inner));
        } else if t.starts_with('#') {
            out.push((at, end, Tok::Heading));
        } else if t.starts_with('>') {
            out.push((at, end, Tok::Comment));
        } else {
            let ind = line.len() - t.len();
            if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
                let m = t.find(' ').unwrap_or(1);
                out.push((at + ind, at + ind + m, Tok::Operator));
            }
            inline_md(line, at, out);
        }
        at += line.len();
    }
}

/// `code`, **strong**, *emphasis*, [links](url) within a line.
fn inline_md(line: &str, at: usize, out: &mut Vec<(usize, usize, Tok)>) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                let e = line[i + 1..].find('`').map_or(b.len(), |n| i + 1 + n + 1);
                out.push((at + i, at + e, Tok::String));
                i = e;
            }
            b'*' | b'_' if b.get(i + 1) == Some(&b[i]) => {
                let pat = &line[i..i + 2];
                let e = line[i + 2..].find(pat).map_or(i + 2, |n| i + 2 + n + 2);
                out.push((at + i, at + e, Tok::Keyword));
                i = e;
            }
            b'[' => {
                if let Some(close) = line[i..].find("](") {
                    let e = line[i + close..].find(')').map_or(b.len(), |n| i + close + n + 1);
                    out.push((at + i, at + i + close + 1, Tok::Function));
                    out.push((at + i + close + 1, at + e, Tok::String));
                    i = e;
                } else {
                    i += 1;
                }
            }
            _ => i += char_len(line, i),
        }
    }
}

fn sql(src: &str, from: usize, to: usize, out: &mut Vec<(usize, usize, Tok)>) {
    let b = src.as_bytes();
    let mut i = from;
    while i < to {
        let c = b[i];
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            let e = src[i..to].find('\n').map_or(to, |n| i + n);
            out.push((i, e, Tok::Comment));
            i = e;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let e = src[i + 2..to].find("*/").map_or(to, |n| i + 2 + n + 2);
            out.push((i, e, Tok::Comment));
            i = e;
        } else if c == b'\'' || c == b'"' {
            let e = string_end(b, i, true).min(to);
            out.push((i, e, if c == b'"' { Tok::Property } else { Tok::String }));
            i = e;
        } else if c.is_ascii_digit() {
            let e = number_end(b, i).min(to);
            out.push((i, e, Tok::Number));
            i = e;
        } else if c == b'$' || c == b':' && b.get(i + 1).is_some_and(u8::is_ascii_alphanumeric) {
            let e = ident_end(b, i + 1, b"").min(to);
            out.push((i, e, Tok::Variable));
            i = e;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let e = ident_end(b, i, b"").min(to);
            let w = src[i..e].to_ascii_lowercase();
            let next = src[e..to].trim_start();
            let tok = if SQL_KEYWORDS.contains(&w.as_str()) { Tok::Keyword } else if next.starts_with('(') { Tok::Function } else { Tok::Text };
            out.push((i, e, tok));
            i = e;
        } else {
            if b"=<>!+-*/|%".contains(&c) {
                out.push((i, i + 1, Tok::Operator));
            }
            i += char_len(src, i);
        }
    }
}

fn directive(src: &str, kind: Kind, out: &mut Vec<(usize, usize, Tok)>) {
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let ind = line.len() - t.len();
        if t.starts_with('#') {
            out.push((at + ind, at + line.trim_end().len(), Tok::Comment));
            at += line.len();
            continue;
        }
        let first = t.split(|c: char| c.is_whitespace() || c == ';' || c == '{').next().unwrap_or("");
        let is_word = match kind {
            Kind::Dockerfile => DOCKER_WORDS.contains(&first.to_ascii_uppercase().as_str()) && ind == 0,
            Kind::Makefile => false,
            _ => !first.is_empty() && first.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        };
        if is_word {
            out.push((at + ind, at + ind + first.len(), Tok::Keyword));
        }
        if kind == Kind::Makefile && ind == 0 {
            if let Some(colon) = t.find(':').filter(|c| !t[..*c].contains('=') && !t[*c..].starts_with(":=")) {
                out.push((at, at + colon, Tok::Function));
            } else if let Some(eq) = t.find('=') {
                out.push((at, at + t[..eq].trim_end_matches([':', '?', '+']).trim_end().len(), Tok::Property));
            }
        }
        // $variables, ${...}, $(...), strings, numbers on the rest of the line.
        let b = line.as_bytes();
        let mut i = ind + if is_word { first.len() } else { 0 };
        while i < b.len() {
            let c = b[i];
            if c == b'"' || c == b'\'' {
                let e = string_end(b, i, false);
                out.push((at + i, at + e, Tok::String));
                i = e;
            } else if c == b'$' {
                let e = if b.get(i + 1) == Some(&b'{') || b.get(i + 1) == Some(&b'(') {
                    let close = if b[i + 1] == b'{' { '}' } else { ')' };
                    line[i..].find(close).map_or(b.len(), |n| i + n + 1)
                } else {
                    ident_end(b, i + 1, b"")
                };
                out.push((at + i, at + e, Tok::Variable));
                i = e;
            } else if c == b'#' && kind != Kind::Makefile {
                out.push((at + i, at + line.trim_end().len(), Tok::Comment));
                break;
            } else if c.is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
                let e = number_end(b, i);
                out.push((at + i, at + e, Tok::Number));
                i = e;
            } else {
                i += char_len(line, i);
            }
        }
        at += line.len();
    }
}

// ---- completion ---------------------------------------------------------------------

const HTML_TAGS: &[&str] = &[
    "a", "abbr", "address", "article", "aside", "audio", "b", "blockquote", "body", "br", "button", "canvas", "caption", "code",
    "col", "colgroup", "data", "datalist", "dd", "details", "dialog", "div", "dl", "dt", "em", "embed", "fieldset", "figcaption",
    "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr", "html", "i", "iframe", "img", "input",
    "label", "legend", "li", "link", "main", "mark", "meta", "meter", "nav", "noscript", "ol", "optgroup", "option", "output", "p",
    "picture", "pre", "progress", "q", "s", "samp", "script", "section", "select", "slot", "small", "source", "span", "strong",
    "style", "sub", "summary", "sup", "svg", "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "time", "title",
    "tr", "track", "u", "ul", "var", "video", "wbr",
];
/// Elements with no closing tag.
pub const VOID_TAGS: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr", "param", "keygen"];
const HTML_ATTRS: &[&str] = &[
    "class", "id", "style", "title", "lang", "dir", "hidden", "tabindex", "role", "aria-label", "aria-hidden", "aria-describedby",
    "data-", "draggable", "contenteditable", "onclick", "oninput", "onchange", "onsubmit", "onkeydown",
];
fn tag_attrs(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" => &["href", "target", "rel", "download"],
        "img" => &["src", "alt", "width", "height", "loading", "decoding", "srcset", "sizes"],
        "input" => &["type", "name", "value", "placeholder", "required", "disabled", "checked", "min", "max", "step", "pattern", "autocomplete", "autofocus", "readonly"],
        "button" => &["type", "disabled", "name", "value", "form"],
        "form" => &["action", "method", "enctype", "novalidate", "autocomplete"],
        "link" => &["rel", "href", "type", "media", "crossorigin", "integrity"],
        "script" => &["src", "type", "async", "defer", "crossorigin", "integrity", "nomodule"],
        "meta" => &["name", "content", "charset", "http-equiv", "property"],
        "label" => &["for"],
        "select" | "textarea" => &["name", "required", "disabled", "multiple", "rows", "cols", "placeholder"],
        "option" => &["value", "selected", "disabled", "label"],
        "video" | "audio" => &["src", "controls", "autoplay", "loop", "muted", "poster", "preload", "playsinline"],
        "iframe" => &["src", "allow", "allowfullscreen", "loading", "sandbox", "referrerpolicy"],
        "td" | "th" => &["colspan", "rowspan", "scope", "headers"],
        "canvas" => &["width", "height"],
        "source" => &["src", "type", "srcset", "media"],
        _ => &[],
    }
}

const CSS_PROPERTIES: &[&str] = &[
    "align-content", "align-items", "align-self", "animation", "aspect-ratio", "backdrop-filter", "background", "background-color",
    "background-image", "background-position", "background-repeat", "background-size", "border", "border-bottom", "border-color",
    "border-left", "border-radius", "border-right", "border-style", "border-top", "border-width", "bottom", "box-shadow",
    "box-sizing", "color", "column-gap", "content", "cursor", "display", "filter", "flex", "flex-basis", "flex-direction",
    "flex-grow", "flex-shrink", "flex-wrap", "float", "font", "font-family", "font-size", "font-style", "font-weight", "gap",
    "grid", "grid-area", "grid-column", "grid-row", "grid-template-areas", "grid-template-columns", "grid-template-rows", "height",
    "inset", "justify-content", "justify-items", "justify-self", "left", "letter-spacing", "line-height", "list-style", "margin",
    "margin-bottom", "margin-left", "margin-right", "margin-top", "max-height", "max-width", "min-height", "min-width", "object-fit",
    "opacity", "outline", "overflow", "overflow-x", "overflow-y", "padding", "padding-bottom", "padding-left", "padding-right",
    "padding-top", "place-items", "pointer-events", "position", "right", "row-gap", "text-align", "text-decoration",
    "text-overflow", "text-shadow", "text-transform", "top", "transform", "transition", "user-select", "vertical-align",
    "visibility", "white-space", "width", "word-break", "z-index",
];
const CSS_COLORS: &[&str] = &[
    "currentColor", "transparent", "black", "white", "red", "green", "blue", "gray", "grey", "orange", "yellow", "purple", "pink",
    "brown", "cyan", "magenta", "navy", "teal", "olive", "maroon", "silver", "gold", "coral", "crimson", "indigo", "lime",
    "salmon", "tomato", "turquoise", "violet", "rgb()", "rgba()", "hsl()", "hsla()", "oklch()", "color-mix()", "var()",
];
fn css_values(prop: &str) -> &'static [&'static str] {
    match prop {
        "display" => &["block", "inline", "inline-block", "flex", "inline-flex", "grid", "inline-grid", "none", "contents", "table"],
        "position" => &["static", "relative", "absolute", "fixed", "sticky"],
        "flex-direction" => &["row", "row-reverse", "column", "column-reverse"],
        "flex-wrap" => &["nowrap", "wrap", "wrap-reverse"],
        "justify-content" | "align-content" => &["flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly", "start", "end", "stretch"],
        "align-items" | "align-self" | "justify-items" | "justify-self" | "place-items" => &["stretch", "center", "start", "end", "flex-start", "flex-end", "baseline"],
        "text-align" => &["left", "right", "center", "justify", "start", "end"],
        "overflow" | "overflow-x" | "overflow-y" => &["visible", "hidden", "scroll", "auto", "clip"],
        "font-weight" => &["normal", "bold", "lighter", "bolder", "100", "200", "300", "400", "500", "600", "700", "800", "900"],
        "font-style" => &["normal", "italic", "oblique"],
        "cursor" => &["pointer", "default", "text", "move", "grab", "grabbing", "not-allowed", "wait", "crosshair", "help"],
        "visibility" => &["visible", "hidden", "collapse"],
        "white-space" => &["normal", "nowrap", "pre", "pre-wrap", "pre-line", "break-spaces"],
        "text-transform" => &["none", "uppercase", "lowercase", "capitalize"],
        "text-decoration" => &["none", "underline", "overline", "line-through"],
        "box-sizing" => &["border-box", "content-box"],
        "object-fit" => &["contain", "cover", "fill", "none", "scale-down"],
        "pointer-events" => &["auto", "none"],
        "user-select" => &["none", "auto", "text", "all"],
        "border-style" => &["solid", "dashed", "dotted", "double", "none", "groove", "ridge", "inset", "outset"],
        "background-repeat" => &["repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round"],
        "background-size" => &["cover", "contain", "auto"],
        "float" => &["left", "right", "none"],
        "word-break" => &["normal", "break-all", "keep-all", "break-word"],
        "vertical-align" => &["baseline", "top", "middle", "bottom", "text-top", "text-bottom"],
        _ if prop.contains("color") || prop == "background" || prop == "fill" || prop == "stroke" || prop == "outline" || prop.starts_with("border") => CSS_COLORS,
        _ => &["auto", "none", "0", "100%", "1rem", "1em", "1px", "calc()", "var()", "min()", "max()", "clamp()"],
    }
}
const CSS_GLOBAL_VALUES: &[&str] = &["inherit", "initial", "unset", "revert"];
const CSS_PSEUDO: &[&str] = &[
    "hover", "focus", "focus-visible", "focus-within", "active", "visited", "disabled", "checked", "first-child", "last-child",
    "nth-child()", "nth-of-type()", "not()", "is()", "where()", "has()", "root", "empty", "placeholder", ":before", ":after",
    ":placeholder", ":selection", ":marker",
];
const CSS_AT: &[&str] = &["media", "supports", "keyframes", "font-face", "import", "layer", "container", "property", "page"];
const SCSS_AT: &[&str] = &["use", "forward", "mixin", "include", "function", "return", "if", "else", "each", "for", "while", "extend", "content", "debug", "warn", "error", "at-root"];

const JS_GLOBALS: &[&str] = &[
    "document", "window", "console", "Math", "JSON", "Object", "Array", "Promise", "Number", "String", "Boolean", "Date", "Map",
    "Set", "WeakMap", "RegExp", "Error", "Symbol", "fetch", "setTimeout", "setInterval", "clearTimeout", "clearInterval",
    "requestAnimationFrame", "localStorage", "sessionStorage", "navigator", "location", "history", "URL", "URLSearchParams",
    "FormData", "Headers", "Response", "Request", "AbortController", "Event", "CustomEvent", "parseInt", "parseFloat",
    "structuredClone", "globalThis", "queueMicrotask", "crypto", "performance", "alert", "confirm", "prompt",
];
/// The members a few well-known objects offer, after `name.`.
fn js_members(obj: &str) -> &'static [&'static str] {
    match obj {
        "document" => &[
            "getElementById()", "querySelector()", "querySelectorAll()", "createElement()", "createTextNode()", "addEventListener()",
            "removeEventListener()", "body", "head", "documentElement", "title", "cookie", "readyState", "activeElement",
            "getElementsByClassName()", "getElementsByTagName()", "createDocumentFragment()", "forms", "images", "links", "hidden",
            "visibilityState", "fonts", "location", "referrer", "URL", "dispatchEvent()",
        ],
        "window" | "globalThis" => &[
            "addEventListener()", "removeEventListener()", "innerWidth", "innerHeight", "outerWidth", "devicePixelRatio", "scrollX",
            "scrollY", "scrollTo()", "open()", "close()", "location", "history", "localStorage", "sessionStorage", "navigator",
            "requestAnimationFrame()", "cancelAnimationFrame()", "setTimeout()", "setInterval()", "matchMedia()", "getComputedStyle()",
            "fetch()", "alert()", "confirm()", "prompt()", "performance", "crypto", "screen", "document",
        ],
        "console" => &["log()", "info()", "warn()", "error()", "debug()", "table()", "dir()", "group()", "groupEnd()", "time()", "timeEnd()", "count()", "assert()", "trace()", "clear()"],
        "Math" => &["abs()", "ceil()", "floor()", "round()", "trunc()", "max()", "min()", "pow()", "sqrt()", "cbrt()", "random()", "sign()", "sin()", "cos()", "tan()", "atan2()", "log()", "log10()", "log2()", "exp()", "hypot()", "PI", "E", "SQRT2", "LN2", "LN10"],
        "JSON" => &["parse()", "stringify()"],
        "Object" => &["keys()", "values()", "entries()", "assign()", "freeze()", "fromEntries()", "create()", "defineProperty()", "getPrototypeOf()", "hasOwn()", "is()"],
        "Array" => &["from()", "isArray()", "of()"],
        "Promise" => &["all()", "allSettled()", "any()", "race()", "resolve()", "reject()", "withResolvers()"],
        "Number" => &["isFinite()", "isInteger()", "isNaN()", "parseFloat()", "parseInt()", "MAX_SAFE_INTEGER", "MIN_SAFE_INTEGER", "EPSILON"],
        "localStorage" | "sessionStorage" => &["getItem()", "setItem()", "removeItem()", "clear()", "key()", "length"],
        "navigator" => &["userAgent", "language", "languages", "clipboard", "onLine", "geolocation", "mediaDevices", "serviceWorker", "share()", "vibrate()", "hardwareConcurrency"],
        "location" => &["href", "pathname", "search", "hash", "host", "hostname", "origin", "port", "protocol", "reload()", "assign()", "replace()"],
        "history" => &["back()", "forward()", "go()", "pushState()", "replaceState()", "state", "length"],
        "JSON.parse" => &[],
        "process" => &["env", "argv", "cwd()", "exit()", "platform", "version", "nextTick()", "stdout", "stderr", "pid"],
        "Date" => &["now()", "parse()", "UTC()"],
        _ => &[],
    }
}
/// Methods any array or string has, offered after a `.` on an unknown name.
const JS_COMMON_MEMBERS: &[&str] = &[
    "length", "map()", "filter()", "reduce()", "forEach()", "find()", "findIndex()", "some()", "every()", "includes()", "indexOf()",
    "slice()", "splice()", "push()", "pop()", "shift()", "unshift()", "concat()", "join()", "split()", "sort()", "reverse()",
    "flat()", "flatMap()", "at()", "keys()", "values()", "entries()", "trim()", "toUpperCase()", "toLowerCase()", "startsWith()",
    "endsWith()", "replace()", "replaceAll()", "padStart()", "padEnd()", "toString()", "toFixed()", "then()", "catch()",
    "finally()", "addEventListener()", "classList", "style", "textContent", "innerHTML", "value", "dataset", "appendChild()",
    "append()", "remove()", "setAttribute()", "getAttribute()", "closest()", "querySelector()", "querySelectorAll()", "json()",
    "text()", "ok", "status", "preventDefault()", "stopPropagation()", "target",
];

/// What may be typed at the cursor in a file of `kind`: the column the
/// replaced word starts at and the choices with a word on what each is.
/// `text` is the whole file (for its own variables, mixins and keys),
/// `before` the cursor's line up to the cursor, `cursor` its byte offset in
/// `text`. None: nothing to offer here (the file's words still are, in
/// edcomplete.rs).
pub fn complete(kind: Kind, text: &str, before: &str, cursor: usize) -> Option<(usize, Vec<(String, &'static str)>)> {
    let cursor = cursor.min(text.len());
    let word_start = |extra: &[char]| before.char_indices().rev().take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || extra.contains(c)).last().map_or(before.chars().count(), |(i, _)| before[..i].chars().count());
    let col = before.chars().count();
    let list = |items: &[&str], what: &'static str| items.iter().map(|s| (s.to_string(), what)).collect::<Vec<_>>();
    match kind.family() {
        Family::Markup | Family::CLike if kind.markup() && in_markup_context(text, cursor, kind) => {
            // Inside an open tag: attributes. After `<`: tags. After `</`: the
            // tag still open, first.
            let start = word_start(&['-', ':']);
            let pre = &before[..before.char_indices().nth(start).map_or(before.len(), |(i, _)| i)];
            if pre.ends_with("</") {
                let mut v = Vec::new();
                if let Some(open) = unclosed_tag(&text[..cursor.saturating_sub(col - start + 2)]) {
                    v.push((format!("{open}>"), "closes the open tag"));
                }
                v.extend(list(HTML_TAGS, "tag"));
                return Some((start, v));
            }
            if pre.ends_with('<') {
                let mut v = list(HTML_TAGS, "tag");
                if kind == Kind::Xml {
                    v = own_tags(text).into_iter().map(|t| (t, "tag in this file")).collect();
                }
                return Some((start, v));
            }
            if let Some(tag) = open_tag_at(before) {
                let mut v = list(tag_attrs(&tag), "attribute");
                v.extend(list(HTML_ATTRS, "global attribute"));
                if matches!(kind, Kind::Jsx | Kind::Tsx) {
                    v.retain(|(a, _)| a != "class");
                    v.insert(0, ("className".into(), "attribute"));
                }
                return Some((start, v));
            }
            if matches!(kind, Kind::Jsx | Kind::Tsx) {
                return js_complete(kind, text, before, word_start(&['$']), col);
            }
            None
        }
        Family::CLike if matches!(kind, Kind::JavaScript | Kind::TypeScript | Kind::Jsx | Kind::Tsx) => js_complete(kind, text, before, word_start(&['$']), col),
        Family::CLike if kind == Kind::Json => {
            let start = word_start(&['-']);
            let mut v: Vec<(String, &'static str)> = json_keys(text).into_iter().map(|k| (k, "key in this file")).collect();
            v.extend(list(&["true", "false", "null"], "value"));
            Some((start, v))
        }
        Family::CLike => {
            let (kw, konst) = keywords(kind);
            let mut v = list(kw, "keyword");
            v.extend(list(konst, "type or constant"));
            Some((word_start(&[]), v))
        }
        Family::Style => style_complete(kind, text, before, cursor, word_start(&['-']), col),
        Family::Sql => Some((word_start(&[]), SQL_KEYWORDS.iter().map(|k| (k.to_ascii_uppercase(), "keyword")).collect())),
        Family::Yaml => Some((word_start(&['-']), yaml_keys(text).into_iter().map(|k| (k, "key in this file")).collect())),
        Family::Directive if kind == Kind::Dockerfile && before.trim_start().len() == before.trim().len() && !before.trim().contains(' ') => Some((word_start(&[]), list(DOCKER_WORDS, "instruction"))),
        Family::Markup if kind == Kind::Php => {
            let (kw, konst) = keywords(kind);
            let mut v = list(kw, "keyword");
            v.extend(list(konst, "constant"));
            Some((word_start(&['$']), v))
        }
        _ => None,
    }
}

fn js_complete(kind: Kind, text: &str, before: &str, start: usize, col: usize) -> Option<(usize, Vec<(String, &'static str)>)> {
    let list = |items: &[&str], what: &'static str| items.iter().map(|s| (s.to_string(), what)).collect::<Vec<_>>();
    let chars: Vec<char> = before.chars().collect();
    if start > 0 && chars.get(start - 1) == Some(&'.') {
        // A member: of a known object, else what arrays, strings and
        // elements have.
        let obj: String = chars[..start - 1].iter().rev().take_while(|c| c.is_alphanumeric() || **c == '_' || **c == '$').collect::<Vec<_>>().into_iter().rev().collect();
        let known = js_members(&obj);
        let v = if known.is_empty() { list(JS_COMMON_MEMBERS, "method or property") } else { list(known, "member") };
        return Some((start, v));
    }
    let _ = (col, text);
    let mut v = list(JS_GLOBALS, "global");
    v.extend(list(JS_KEYWORDS, "keyword"));
    v.extend(list(JS_CONSTANTS, "constant"));
    if matches!(kind, Kind::TypeScript | Kind::Tsx) {
        v.extend(list(TS_KEYWORDS, "keyword"));
        v.extend(list(TS_TYPES, "type"));
    }
    Some((start, v))
}

fn style_complete(kind: Kind, text: &str, before: &str, cursor: usize, start: usize, col: usize) -> Option<(usize, Vec<(String, &'static str)>)> {
    let list = |items: &[&str], what: &'static str| items.iter().map(|s| (s.to_string(), what)).collect::<Vec<_>>();
    let chars: Vec<char> = before.chars().collect();
    let scss = matches!(kind, Kind::Scss | Kind::Sass | Kind::Less);
    // $variables (SCSS) and @variables (Less) defined in the file.
    if start > 0 && (chars[start - 1] == '$' || kind == Kind::Less && chars[start - 1] == '@' && depth_at(text, cursor) > 0) {
        let sigil = chars[start - 1];
        let v = style_vars(text, sigil).into_iter().map(|n| (n, "variable in this file")).collect();
        return Some((start, v));
    }
    if start > 0 && chars[start - 1] == '@' {
        let mut v = list(CSS_AT, "at-rule");
        if scss {
            v.extend(list(SCSS_AT, "SCSS directive"));
        }
        return Some((start, v));
    }
    let line: String = chars.iter().collect();
    // This statement: what follows the last ';', '{' or '}' on the line.
    let stmt = line.rsplit([';', '{', '}']).next().unwrap_or(&line);
    if scss && stmt.trim_start().starts_with("@include ") {
        let v = mixins(text).into_iter().map(|n| (n, "mixin in this file")).collect();
        return Some((start, v));
    }
    let inside = depth_at(text, cursor) > 0;
    // A value: after `property:` on this statement.
    if inside {
        if let Some((prop, _)) = stmt.split_once(':') {
            let prop = prop.trim();
            if !prop.is_empty() && !prop.contains(' ') && !prop.starts_with(['&', '.', '#']) && !stmt.trim_end().ends_with('{') {
                let mut v = list(css_values(prop), "value");
                v.extend(list(CSS_GLOBAL_VALUES, "keyword"));
                if scss {
                    v.extend(style_vars(text, '$').into_iter().map(|n| (format!("${n}"), "variable in this file")));
                }
                return Some((start, v));
            }
        }
    }
    // A pseudo-class: after ':' in a selector.
    if start > 0 && chars[start - 1] == ':' && (!inside || stmt.trim_end().ends_with([':']) && stmt.trim_start().starts_with('&')) {
        return Some((start, list(CSS_PSEUDO, "pseudo-class")));
    }
    if inside {
        let mut v = list(CSS_PROPERTIES, "property");
        if scss {
            v.extend(list(HTML_TAGS, "selector"));
        }
        return Some((start, v));
    }
    let _ = col;
    Some((start, list(HTML_TAGS, "selector")))
}

/// How many `{` are open at `cursor`, comments and strings aside (roughly).
fn depth_at(text: &str, cursor: usize) -> i32 {
    let mut d = 0;
    let mut in_comment = false;
    let b = text.as_bytes();
    let mut i = 0;
    while i < cursor.min(b.len()) {
        if in_comment {
            if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                in_comment = false;
                i += 1;
            }
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            in_comment = true;
        } else if b[i] == b'{' {
            d += 1;
        } else if b[i] == b'}' {
            d -= 1;
        }
        i += 1;
    }
    d
}

fn style_vars(text: &str, sigil: char) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix(sigil) {
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            if !name.is_empty() && rest[name.len()..].trim_start().starts_with(':') && !v.contains(&name) {
                v.push(name);
            }
        }
    }
    v
}

fn mixins(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim_start().strip_prefix("@mixin "))
        .map(|r| r.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>())
        .filter(|n| !n.is_empty())
        .collect()
}

fn json_keys(text: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (a, b, t) in tokens(text, Kind::Json) {
        if t == Tok::Property {
            let k = text[a..b].to_owned();
            if !v.contains(&k) {
                v.push(k);
            }
        }
    }
    v
}

fn yaml_keys(text: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (a, b, t) in tokens(text, Kind::Yaml) {
        if t == Tok::Property {
            let k = text[a..b].trim().to_owned();
            if !k.is_empty() && !v.contains(&k) {
                v.push(k);
            }
        }
    }
    v
}

fn own_tags(text: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        let name: String = rest[i + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == ':' || *c == '_').collect();
        if !name.is_empty() && !v.contains(&name) {
            v.push(name);
        }
        rest = &rest[i + 1..];
    }
    v
}

/// Is the cursor where markup is written - not inside a <script>/<style>
/// body or a PHP block? (JSX: always, the caller decides.)
fn in_markup_context(text: &str, cursor: usize, kind: Kind) -> bool {
    if matches!(kind, Kind::Jsx | Kind::Tsx) {
        return true;
    }
    let low = text[..cursor].to_ascii_lowercase();
    let opened = |o: &str, c: &str| low.rfind(o).is_some_and(|i| low[i..].find(c).is_none());
    !(opened("<script", "</script") && low.rfind("<script").is_some_and(|i| low[i..].contains('>')) || opened("<style", "</style") && low.rfind("<style").is_some_and(|i| low[i..].contains('>')) || opened("<?php", "?>"))
}

/// The tag whose attributes the cursor is among: `<a href="x" |`.
fn open_tag_at(before: &str) -> Option<String> {
    let lt = before.rfind('<')?;
    let after = &before[lt + 1..];
    if after.contains('>') || after.starts_with('/') || after.starts_with('!') {
        return None;
    }
    // In an attribute's value, not its name.
    if after.matches('"').count() % 2 == 1 || after.matches('\'').count() % 2 == 1 {
        return None;
    }
    let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '-').collect();
    (!name.is_empty() && after.len() > name.len()).then_some(name.to_ascii_lowercase())
}

/// The innermost tag left open in `text` (void and self-closed tags aside).
pub fn unclosed_tag(text: &str) -> Option<String> {
    let mut stack: Vec<String> = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if text[i..].starts_with("<!--") {
            i = text[i..].find("-->").map_or(b.len(), |n| i + n + 3);
            continue;
        }
        if b[i] == b'<' {
            let e = tag_end(b, i, b.len());
            let inner = &text[i + 1..e.saturating_sub(1).max(i + 1)];
            if let Some(name) = inner.strip_prefix('/') {
                let name: String = name.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == ':' || *c == '.').collect();
                if let Some(p) = stack.iter().rposition(|t| t.eq_ignore_ascii_case(&name)) {
                    stack.truncate(p);
                }
            } else if !inner.starts_with(['!', '?']) && e <= b.len() && text[i..e].ends_with('>') {
                let name: String = inner.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == ':' || *c == '.').collect();
                if !name.is_empty() && !inner.trim_end().ends_with('/') && !VOID_TAGS.contains(&name.to_ascii_lowercase().as_str()) {
                    stack.push(name);
                }
            }
            i = e;
            continue;
        }
        i += char_len(text, i);
    }
    stack.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_of(src: &str, kind: Kind) -> Vec<(String, Tok)> {
        tokens(src, kind).into_iter().filter(|t| t.2 != Tok::Text).map(|(a, b, t)| (src[a..b].to_owned(), t)).collect()
    }

    fn has(src: &str, kind: Kind, piece: &str, tok: Tok) -> bool {
        kinds_of(src, kind).iter().any(|(s, t)| s == piece && *t == tok)
    }

    #[test]
    fn files_are_known_by_name_extension_and_first_line() {
        assert_eq!(of_path("/d/app/web/main.js"), Some(Kind::JavaScript));
        assert_eq!(of_path("a/b/App.tsx"), Some(Kind::Tsx));
        assert_eq!(of_path("style.scss"), Some(Kind::Scss));
        assert_eq!(of_path("x/Dockerfile"), Some(Kind::Dockerfile));
        assert_eq!(of_path("nginx/conf.d/common_proxy.conf"), Some(Kind::Nginx));
        assert_eq!(of_path("Cargo.toml"), Some(Kind::Toml));
        assert_eq!(of_path("notes.txt"), None);
        assert_eq!(of_text("<!doctype html>\n<html>"), Some(Kind::Html));
        assert_eq!(of_text("server {\n  listen 443 ssl;\n}"), Some(Kind::Nginx));
        assert_eq!(of_text("{\"a\": 1}"), Some(Kind::Json));
    }

    #[test]
    fn tokens_cover_every_byte_in_order() {
        for (src, kind) in [
            ("const a = `x${b}` // é\nlet c = 'd';", Kind::JavaScript),
            ("<p class=\"x\">é &amp; <b>y</b></p><script>let a=1</script><style>a{color:red}</style>", Kind::Html),
            ("$x: 1px;\n.a { color: $x; &:hover { top: 0 } }", Kind::Scss),
            ("a: 1\nb:\n  - \"c\" # d\n", Kind::Yaml),
            ("# T\n```js\nlet x = 1\n```\n*a* **b** `c` [d](e)", Kind::Markdown),
            ("SELECT * FROM t -- x\nWHERE a = 'b';", Kind::Sql),
        ] {
            let t = tokens(src, kind);
            let mut at = 0;
            for (a, b, _) in &t {
                assert_eq!(*a, at, "{kind:?}");
                assert!(b > a);
                at = *b;
            }
            assert_eq!(at, src.len(), "{kind:?}");
        }
    }

    #[test]
    fn javascript_and_typescript_colour_as_their_parts() {
        let src = "import { x } from \"./y.js\";\nconst n = 42; // the answer\nfunction go(a) { return a ? true : null; }";
        assert!(has(src, Kind::JavaScript, "import", Tok::Keyword));
        assert!(has(src, Kind::JavaScript, "\"./y.js\"", Tok::String));
        assert!(has(src, Kind::JavaScript, "42", Tok::Number));
        assert!(has(src, Kind::JavaScript, "// the answer", Tok::Comment));
        assert!(has(src, Kind::JavaScript, "go", Tok::Function));
        assert!(has(src, Kind::JavaScript, "true", Tok::Constant));
        assert!(has("let a: string = x as Foo;", Kind::TypeScript, "string", Tok::Constant));
        assert!(has("let a: string = x as Foo;", Kind::TypeScript, "Foo", Tok::Constant));
        let jsx = "return (<div className=\"a\" onClick={() => go(1)}>hi</div>);";
        assert!(has(jsx, Kind::Jsx, "<div", Tok::Tag));
        assert!(has(jsx, Kind::Jsx, "className", Tok::Attr));
        assert!(has(jsx, Kind::Jsx, "</div", Tok::Tag));
        assert!(has("{\"name\": \"x\", \"n\": 2}", Kind::Json, "\"name\"", Tok::Property));
        assert!(has("{\"name\": \"x\", \"n\": 2}", Kind::Json, "\"x\"", Tok::String));
    }

    #[test]
    fn html_colours_tags_attributes_and_embedded_code() {
        let src = "<!-- c --><a href=\"/x\" data-k='v'>t</a><script>const y = 1;</script><style>p { color: red; }</style>";
        assert!(has(src, Kind::Html, "<!-- c -->", Tok::Comment));
        assert!(has(src, Kind::Html, "<a", Tok::Tag));
        assert!(has(src, Kind::Html, "href", Tok::Attr));
        assert!(has(src, Kind::Html, "\"/x\"", Tok::String));
        assert!(has(src, Kind::Html, "data-k", Tok::Attr));
        assert!(has(src, Kind::Html, "const", Tok::Keyword));
        assert!(has(src, Kind::Html, "color", Tok::Property));
        assert!(has("<?php echo $name; ?>", Kind::Php, "$name", Tok::Variable));
        assert!(has("<?php echo $name; ?>", Kind::Php, "echo", Tok::Keyword));
    }

    #[test]
    fn stylesheets_tell_selectors_properties_values_and_variables_apart() {
        let src = "$primary: #3a7;\n@mixin card { padding: 1rem; }\n.btn:hover { color: $primary; margin: 0 auto !important; }";
        assert!(has(src, Kind::Scss, "$primary", Tok::Variable));
        assert!(has(src, Kind::Scss, "@mixin", Tok::Keyword));
        assert!(has(src, Kind::Scss, ".btn", Tok::Tag));
        assert!(has(src, Kind::Scss, "color", Tok::Property));
        assert!(has(src, Kind::Scss, "1rem", Tok::Number));
        assert!(has(src, Kind::Scss, "auto", Tok::Constant));
        assert!(has(src, Kind::Scss, "!important", Tok::Keyword));
        assert!(has("a:hover { top: 0 }", Kind::Css, "a", Tok::Tag));
    }

    #[test]
    fn data_and_config_formats_colour_their_keys() {
        assert!(has("name: cove\nport: 8080\non: true\n", Kind::Yaml, "name", Tok::Property));
        assert!(has("name: cove\nport: 8080\non: true\n", Kind::Yaml, "8080", Tok::Number));
        assert!(has("[package]\nname = \"x\"\n", Kind::Toml, "[package]", Tok::Tag));
        assert!(has("[package]\nname = \"x\"\n", Kind::Toml, "name", Tok::Property));
        assert!(has("server {\n  listen 443 ssl; # tls\n  root $root;\n}", Kind::Nginx, "listen", Tok::Keyword));
        assert!(has("server {\n  listen 443 ssl; # tls\n  root $root;\n}", Kind::Nginx, "$root", Tok::Variable));
        assert!(has("FROM debian:12\nRUN apt update", Kind::Dockerfile, "RUN", Tok::Keyword));
        assert!(has("select a from t where b = 'c'", Kind::Sql, "select", Tok::Keyword));
        assert!(has("# Title\n- item\n", Kind::Markdown, "# Title", Tok::Heading));
        assert!(has("```rust\nfn x() {}\n```", Kind::Markdown, "fn", Tok::Keyword));
        assert!(has("int main() { return 0; }", Kind::C, "int", Tok::Keyword));
        assert!(has("public class A { }", Kind::Java, "class", Tok::Keyword));
        assert!(has("namespace X { class Y {} }", Kind::CSharp, "namespace", Tok::Keyword));
    }

    fn offered(kind: Kind, text: &str) -> Vec<String> {
        // `|` marks the cursor.
        let at = text.find('|').unwrap();
        let t = text.replace('|', "");
        let line_start = t[..at].rfind('\n').map_or(0, |i| i + 1);
        complete(kind, &t, &t[line_start..at], at).map(|(_, v)| v.into_iter().map(|x| x.0).collect()).unwrap_or_default()
    }

    #[test]
    fn html_offers_tags_attributes_and_the_closing_tag() {
        assert!(offered(Kind::Html, "<di|").contains(&"div".to_owned()));
        let attrs = offered(Kind::Html, "<a hr|");
        assert!(attrs.contains(&"href".to_owned()) && attrs.contains(&"class".to_owned()));
        assert_eq!(offered(Kind::Html, "<ul><li>x</li></|")[0], "ul>");
        assert!(offered(Kind::Html, "<script>docu|").is_empty(), "in a script: not tags");
    }

    #[test]
    fn css_offers_values_for_the_property_and_scss_its_variables() {
        let colors = offered(Kind::Css, "a { color: |");
        assert!(colors.contains(&"red".to_owned()) && colors.contains(&"currentColor".to_owned()) && colors.contains(&"inherit".to_owned()));
        assert!(offered(Kind::Css, "a { disp|").contains(&"display".to_owned()));
        assert!(offered(Kind::Css, "a { display: |").contains(&"flex".to_owned()));
        assert!(offered(Kind::Css, "a:hov|").contains(&"hover".to_owned()));
        let scss = "$primary: #3a7;\n$gap: 4px;\n.a { color: $|";
        let vars = offered(Kind::Scss, scss);
        assert!(vars.contains(&"primary".to_owned()) && vars.contains(&"gap".to_owned()));
        assert!(offered(Kind::Scss, "@mixin card {}\n.a { @include |").contains(&"card".to_owned()));
    }

    #[test]
    fn javascript_offers_the_members_of_what_is_before_the_dot() {
        let d = offered(Kind::JavaScript, "document.|");
        assert!(d.contains(&"querySelector()".to_owned()) && d.contains(&"getElementById()".to_owned()));
        assert!(offered(Kind::JavaScript, "console.l|").contains(&"log()".to_owned()));
        assert!(offered(Kind::JavaScript, "items.|").contains(&"map()".to_owned()));
        let ts = offered(Kind::TypeScript, "let a: str|");
        assert!(ts.contains(&"string".to_owned()) && ts.contains(&"interface".to_owned()));
        assert!(offered(Kind::Jsx, "<div clas|").contains(&"className".to_owned()));
        assert!(offered(Kind::Json, "{\"name\": 1,\n \"n|").contains(&"\"name\"".to_owned()));
    }

    #[test]
    fn the_innermost_open_tag_is_found() {
        assert_eq!(unclosed_tag("<div><p>a</p><span class=\"x\">").as_deref(), Some("span"));
        assert_eq!(unclosed_tag("<div><br><img src=x></div>"), None);
        assert_eq!(unclosed_tag("<ul><li>a</li>").as_deref(), Some("ul"));
    }
}
