//! Generates a typed message catalog from `i18n/*.ftl`.
//!
//! Why this exists rather than a runtime lookup (`t("login-password")`): a typo
//! in a message id should break `cargo build`, in every template that used it,
//! the same way a renamed database column does. Runtime id resolution turns the
//! same mistake into a blank space on a page nobody happened to open.
//!
//! What it emits, per message:
//!
//!   * no placeables, no selector -> `fn x(&self) -> &'static str`  (borrowed,
//!     zero allocation — the ledger renders hundreds of these per request)
//!   * placeables, no selector    -> `fn x(&self, a: &str) -> String` via format!
//!   * a plural selector          -> the ONLY path that touches FluentBundle
//!
//! The build fails — loudly, with every problem listed at once — on a duplicate
//! id, an id present in one locale but not the other, a differing placeable set
//! for the same id, a mangled-name collision, or forbidden content (§ scan_text).

use fluent_syntax::ast;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

/// `en` is the reference locale: it fixes each message's parameter order, so the
/// two files can never disagree about arity or ordering.
const REFERENCE: &str = "en";
const LOCALES: &[(&str, &str)] = &[("en", "En"), ("pt-BR", "PtBr")];

fn main() {
    println!("cargo:rerun-if-changed=i18n");
    for (code, _) in LOCALES {
        println!("cargo:rerun-if-changed=i18n/{code}.ftl");
    }

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut errors: Vec<String> = Vec::new();
    let mut catalogs: BTreeMap<&str, BTreeMap<String, Msg>> = BTreeMap::new();

    for (code, _) in LOCALES {
        let path = root.join("i18n").join(format!("{code}.ftl"));
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| fail(&format!("i18n/{code}.ftl: {e}")));
        catalogs.insert(code, parse_locale(code, &src, &mut errors));
    }

    cross_check(&catalogs, &mut errors);

    if !errors.is_empty() {
        // Every problem at once. Fixing one id per build cycle is how a catalog
        // with a hundred missing translations takes a hundred builds.
        fail(&format!(
            "the message catalog has {} problem(s):\n\n  {}\n",
            errors.len(),
            errors.join("\n  ")
        ));
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("catalog.rs");
    std::fs::write(&out, generate(&catalogs, &root)).unwrap();
}

// ---------------------------------------------------------------------------
// model
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Msg {
    /// Variable names in order of first appearance — used only from the
    /// reference locale, which is what pins the generated parameter order.
    order: Vec<String>,
    /// The same names as a set, for comparing locales without caring about order.
    vars: BTreeSet<String>,
    /// A selector means plural rules, which means the FluentBundle fallback.
    selector: bool,
    /// Elements flattened to (text, is_placeholder_var) for codegen.
    parts: Vec<Part>,
}

#[derive(Clone)]
enum Part {
    Text(String),
    Var(String),
}

// ---------------------------------------------------------------------------
// parse + per-locale validation
// ---------------------------------------------------------------------------

fn parse_locale(code: &str, src: &str, errors: &mut Vec<String>) -> BTreeMap<String, Msg> {
    let res = match fluent_syntax::parser::parse(src) {
        Ok(r) => r,
        Err((r, errs)) => {
            for e in errs {
                let (line, col) = line_col(src, e.pos.start);
                errors.push(format!("{code}.ftl:{line}:{col} — {}", e.kind));
            }
            r
        }
    };

    let mut out: BTreeMap<String, Msg> = BTreeMap::new();

    for entry in &res.body {
        let ast::Entry::Message(m) = entry else {
            // Comments and terms carry no accessor. Note comments are skipped
            // here entirely, which is why a comment may legitimately read
            // "formatted by fmt::money, e.g. R$ 1.234,56" without tripping the
            // forbidden-content scan below.
            continue;
        };
        let id = m.id.name.to_string();

        // Fluent's own parser does NOT reject a repeated id — the later one
        // silently wins, which is a translation quietly disappearing.
        if let Some(value) = &m.value {
            let key = id.clone();
            let msg = collect(value, code, &key, errors);
            if out.insert(key.clone(), msg).is_some() {
                errors.push(format!("{code}.ftl — duplicate message id `{key}`"));
            }
        }
        for attr in &m.attributes {
            let key = format!("{id}.{}", attr.id.name);
            let msg = collect(&attr.value, code, &key, errors);
            if out.insert(key.clone(), msg).is_some() {
                errors.push(format!("{code}.ftl — duplicate message id `{key}`"));
            }
        }
        if m.value.is_none() && m.attributes.is_empty() {
            errors.push(format!(
                "{code}.ftl — `{id}` has no value and no attributes"
            ));
        }
    }

    out
}

fn collect(pattern: &ast::Pattern<&str>, code: &str, key: &str, errors: &mut Vec<String>) -> Msg {
    let mut msg = Msg::default();
    walk_pattern(pattern, &mut msg, code, key, errors, true);
    msg
}

fn walk_pattern(
    pattern: &ast::Pattern<&str>,
    msg: &mut Msg,
    code: &str,
    key: &str,
    errors: &mut Vec<String>,
    top: bool,
) {
    for el in &pattern.elements {
        match el {
            ast::PatternElement::TextElement { value } => {
                // THE SCAN RUNS HERE AND NOWHERE ELSE: on text element contents
                // only, never on raw file bytes. Fluent's variable syntax is
                // `$name`, so a naive scan of the source for `$` rejects the
                // entire catalog on its first interpolated message — and the
                // fix under pressure is always to weaken the check.
                scan_text(value, code, key, errors);
                if top {
                    msg.parts.push(Part::Text((*value).to_string()));
                }
            }
            ast::PatternElement::Placeable { expression } => {
                walk_expr(expression, msg, code, key, errors, top);
            }
        }
    }
}

fn walk_expr(
    expr: &ast::Expression<&str>,
    msg: &mut Msg,
    code: &str,
    key: &str,
    errors: &mut Vec<String>,
    top: bool,
) {
    match expr {
        ast::Expression::Select { selector, variants } => {
            msg.selector = true;
            walk_inline(selector, msg, code, key, errors, false);
            for v in variants {
                // Variant bodies still contribute variables and still get
                // scanned — a currency glyph hidden in the `*[other]` branch is
                // exactly as wrong as one in the main value.
                walk_pattern(&v.value, msg, code, key, errors, false);
            }
        }
        ast::Expression::Inline(inline) => walk_inline(inline, msg, code, key, errors, top),
    }
}

fn walk_inline(
    inline: &ast::InlineExpression<&str>,
    msg: &mut Msg,
    code: &str,
    key: &str,
    errors: &mut Vec<String>,
    top: bool,
) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            let name = id.name.to_string();
            if msg.vars.insert(name.clone()) {
                msg.order.push(name.clone());
            }
            if top {
                msg.parts.push(Part::Var(name));
            }
        }
        ast::InlineExpression::FunctionReference { id, .. } => {
            // NUMBER() and DATETIME() would localize a number or a date inside
            // the catalog, which is precisely the fusion of the three display
            // axes that CLAUDE.md's Conventions section forbids. Money is
            // formatted by ui::fmt::money from the household's currency, dates
            // are fixed dd/mm/yyyy — both are passed in already rendered.
            if matches!(id.name, "NUMBER" | "DATETIME") {
                errors.push(format!(
                    "{code}.ftl — `{key}` calls {}(). Money and dates are formatted by \
                     ui::fmt::money / ui::fmt::date and passed in as &str.",
                    id.name
                ));
            }
        }
        ast::InlineExpression::Placeable { expression } => {
            walk_expr(expression, msg, code, key, errors, top)
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// forbidden content
// ---------------------------------------------------------------------------

fn scan_text(text: &str, code: &str, key: &str, errors: &mut Vec<String>) {
    let mut bad = |what: &str| {
        errors.push(format!(
            "{code}.ftl — `{key}` contains {what}. The catalog holds words only: \
             money comes from ui::fmt::money (household currency) and dates from \
             ui::fmt::date (fixed dd/mm/yyyy), both interpolated as &str."
        ));
    };

    for glyph in ["R$", "US$", "€", "£", "¥"] {
        if text.contains(glyph) {
            bad(&format!("the currency glyph `{glyph}`"));
        }
    }
    if has_currency_adjacent_digit(text) {
        bad("a currency symbol or ISO code next to a digit");
    }
    if has_date_shape(text) {
        bad("a dd/mm/yyyy-shaped date literal");
    }
    if has_grouped_number(text) {
        bad("a thousands-grouped number (e.g. 1.234,56)");
    }
}

/// `$5`, `5 $`, `BRL 10`, `10 USD` — a bare symbol or ISO code doing money's job.
///
/// "Adjacent" ignores intervening spaces on purpose: `R$ 10` and `10 USD` are
/// both money, and only the digit's nearest non-space neighbor matters.
fn has_currency_adjacent_digit(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();

    let digit_left = |i: usize| {
        chars[..i]
            .iter()
            .rev()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c.is_ascii_digit())
    };
    let digit_right = |i: usize| {
        chars[i + 1..]
            .iter()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c.is_ascii_digit())
    };

    if chars
        .iter()
        .enumerate()
        .any(|(i, c)| *c == '$' && (digit_left(i) || digit_right(i)))
    {
        return true;
    }

    for code in ["BRL", "USD", "EUR", "GBP"] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(code) {
            let at = from + rel;
            let before = text[..at].chars().rev().find(|c| !c.is_whitespace());
            let after = text[at + code.len()..].chars().find(|c| !c.is_whitespace());
            if before.is_some_and(|c| c.is_ascii_digit())
                || after.is_some_and(|c| c.is_ascii_digit())
            {
                return true;
            }
            from = at + code.len();
        }
    }
    false
}

/// `dd/mm/yyyy`, `DD/MM/YYYY`, `%d/%m/%Y`, or a real `11/07/2026`.
fn has_date_shape(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("dd/mm/yyyy") || lower.contains("%d/%m/%y") {
        return true;
    }
    let c: Vec<char> = text.chars().collect();
    let digit = |i: usize| c.get(i).is_some_and(|c| c.is_ascii_digit());
    c.len() >= 10
        && (0..=c.len().saturating_sub(10)).any(|i| {
            digit(i)
                && digit(i + 1)
                && c[i + 2] == '/'
                && digit(i + 3)
                && digit(i + 4)
                && c[i + 5] == '/'
                && digit(i + 6)
                && digit(i + 7)
                && digit(i + 8)
                && digit(i + 9)
        })
}

/// `1.234,56` / `1,234.56` — a number already wearing a locale's grouping.
fn has_grouped_number(text: &str) -> bool {
    let c: Vec<char> = text.chars().collect();
    let digit = |i: usize| c.get(i).is_some_and(|ch| ch.is_ascii_digit());
    let sep = |i: usize| matches!(c.get(i), Some('.') | Some(','));
    (0..c.len()).any(|i| {
        digit(i)
            && sep(i + 1)
            && digit(i + 2)
            && digit(i + 3)
            && digit(i + 4)
            && sep(i + 5)
            && digit(i + 6)
            && digit(i + 7)
    })
}

// ---------------------------------------------------------------------------
// cross-locale validation
// ---------------------------------------------------------------------------

fn cross_check(catalogs: &BTreeMap<&str, BTreeMap<String, Msg>>, errors: &mut Vec<String>) {
    let Some(reference) = catalogs.get(REFERENCE) else {
        return;
    };

    for (code, msgs) in catalogs {
        if *code == REFERENCE {
            continue;
        }
        for key in reference.keys() {
            if !msgs.contains_key(key) {
                errors.push(format!(
                    "`{key}` is in {REFERENCE}.ftl but missing from {code}.ftl"
                ));
            }
        }
        for key in msgs.keys() {
            if !reference.contains_key(key) {
                errors.push(format!(
                    "`{key}` is in {code}.ftl but not in the reference locale {REFERENCE}.ftl"
                ));
            }
        }
        for (key, msg) in msgs {
            if let Some(r) = reference.get(key)
                && r.vars != msg.vars
            {
                errors.push(format!(
                    "`{key}` takes {{{}}} in {REFERENCE}.ftl but {{{}}} in {code}.ftl",
                    r.vars.iter().cloned().collect::<Vec<_>>().join(", "),
                    msg.vars.iter().cloned().collect::<Vec<_>>().join(", "),
                ));
            }
        }
    }

    // `a-b.c` and `a-b-c` both mangle to `a_b_c`, which would be two accessors
    // with one name — a compile error far from its cause unless caught here.
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for key in reference.keys() {
        let name = mangle(key);
        if let Some(prev) = seen.insert(name.clone(), key.clone()) {
            errors.push(format!(
                "`{prev}` and `{key}` both become `{name}()` — rename one"
            ));
        }
        if RUST_KEYWORDS.contains(&name.as_str()) {
            errors.push(format!(
                "`{key}` becomes `{name}()`, which is a Rust keyword"
            ));
        }
        if name.starts_with(|c: char| c.is_ascii_digit()) {
            errors.push(format!(
                "`{key}` becomes `{name}()`, which starts with a digit"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// codegen
// ---------------------------------------------------------------------------

fn generate(catalogs: &BTreeMap<&str, BTreeMap<String, Msg>>, root: &std::path::Path) -> String {
    let reference = &catalogs[REFERENCE];
    let mut s = String::new();

    s.push_str("// GENERATED by build.rs from i18n/*.ftl. Do not edit.\n\n");

    // ---- Lang ----
    // The reference locale is the Default: it is the one guaranteed complete,
    // and it is what a request resolves to when nothing else says otherwise.
    s.push_str("#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, Default)]\npub enum Lang {\n");
    for (i, (_, variant)) in LOCALES.iter().enumerate() {
        if i == 0 {
            s.push_str("    #[default]\n");
        }
        let _ = writeln!(s, "    {variant},");
    }
    s.push_str("}\n\n#[allow(dead_code)]\nimpl Lang {\n");
    let _ = writeln!(
        s,
        "    pub const ALL: [Lang; {}] = [{}];",
        LOCALES.len(),
        LOCALES
            .iter()
            .map(|(_, v)| format!("Lang::{v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    s.push_str("    pub fn code(self) -> &'static str {\n        match self {\n");
    for (code, variant) in LOCALES {
        let _ = writeln!(s, "            Lang::{variant} => {},", lit(code));
    }
    s.push_str("        }\n    }\n");
    s.push_str("    pub fn parse(s: &str) -> Option<Lang> {\n        match s {\n");
    for (code, variant) in LOCALES {
        let _ = writeln!(s, "            {} => Some(Lang::{variant}),", lit(code));
    }
    s.push_str("            _ => None,\n        }\n    }\n}\n\n");

    // ---- ids, for the sandbox's coverage page ----
    let _ = writeln!(s, "pub const MESSAGE_IDS: [&str; {}] = [", reference.len());
    for key in reference.keys() {
        let _ = writeln!(s, "    {},", lit(key));
    }
    s.push_str("];\n\n");

    // ---- Catalog ----
    s.push_str("#[derive(Copy, Clone, Debug)]\npub struct Catalog {\n    pub lang: Lang,\n}\n\n");
    // A catalog holds every string the app can say, so most accessors are
    // unused at any given moment — that is the normal state, not a defect, and
    // warning about it would train the warning to be ignored. The sandbox's
    // /__sandbox/catalog page is what actually checks coverage.
    s.push_str("#[allow(dead_code)]\n");
    s.push_str("impl Catalog {\n    pub fn new(lang: Lang) -> Self { Self { lang } }\n\n");

    for (key, r) in reference {
        let name = mangle(key);
        let params: Vec<(String, bool)> =
            r.order.iter().map(|v| (mangle(v), is_count(v))).collect();
        let sig_params = params
            .iter()
            .map(|(n, count)| format!(", {n}: {}", if *count { "i64" } else { "&str" }))
            .collect::<String>();

        if r.selector {
            // The only path that touches FluentBundle.
            let _ = writeln!(
                s,
                "    #[inline]\n    pub fn {name}(&self{sig_params}) -> String {{"
            );
            s.push_str("        let mut args = ::fluent_bundle::FluentArgs::new();\n");
            for (v, (pname, _)) in r.order.iter().zip(params.iter()) {
                let _ = writeln!(
                    s,
                    "        args.set({}, ::fluent_bundle::FluentValue::from({pname}));",
                    lit(v)
                );
            }
            let _ = writeln!(
                s,
                "        __select(self.lang, {}, args)\n    }}\n",
                lit(key)
            );
        } else if r.vars.is_empty() {
            // Constant: borrowed, no allocation, no format machinery.
            let _ = writeln!(
                s,
                "    #[inline]\n    pub fn {name}(&self) -> &'static str {{"
            );
            s.push_str("        match self.lang {\n");
            for (code, variant) in LOCALES {
                let text = catalogs[code]
                    .get(key)
                    .map(constant_text)
                    .unwrap_or_default();
                let _ = writeln!(s, "            Lang::{variant} => {},", lit(&text));
            }
            s.push_str("        }\n    }\n\n");
        } else {
            let _ = writeln!(
                s,
                "    #[inline]\n    pub fn {name}(&self{sig_params}) -> String {{"
            );
            s.push_str("        match self.lang {\n");
            for (code, variant) in LOCALES {
                let msg = &catalogs[code][key];
                let (fmt, args) = format_parts(msg);
                let _ = writeln!(
                    s,
                    "            Lang::{variant} => format!({}{}),",
                    lit(&fmt),
                    args
                );
            }
            s.push_str("        }\n    }\n\n");
        }
    }
    s.push_str("}\n\n");

    selector_runtime(&mut s, root);
    s
}

fn constant_text(m: &Msg) -> String {
    m.parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.as_str(),
            Part::Var(_) => "",
        })
        .collect()
}

/// Builds a `format!` string plus its positional argument list. Positional
/// rather than named because a Fluent variable may legitimately be `$due-date`,
/// which is not a Rust identifier; and because a variable used twice simply
/// gets passed twice, which is free for `&str` and `i64`.
fn format_parts(m: &Msg) -> (String, String) {
    let mut fmt = String::new();
    let mut args = String::new();
    for part in &m.parts {
        match part {
            Part::Text(t) => {
                for ch in t.chars() {
                    match ch {
                        '{' => fmt.push_str("{{"),
                        '}' => fmt.push_str("}}"),
                        _ => fmt.push(ch),
                    }
                }
            }
            Part::Var(v) => {
                fmt.push_str("{}");
                let _ = write!(args, ", {}", mangle(v));
            }
        }
    }
    (fmt, args)
}

fn selector_runtime(s: &mut String, root: &std::path::Path) {
    s.push_str(
        "// ---- plural-selector fallback ------------------------------------------\n\
         // Reached only by messages that actually carry a selector. If the catalog\n\
         // has none, this is dead code — which is the intended steady state.\n\
         type __Bundle = ::fluent_bundle::concurrent::FluentBundle<::fluent_bundle::FluentResource>;\n\n",
    );
    for (code, variant) in LOCALES {
        let path = root.join("i18n").join(format!("{code}.ftl"));
        let _ = writeln!(
            s,
            "const __SRC_{}: &str = include_str!({});",
            variant.to_uppercase(),
            lit(&path.to_string_lossy())
        );
    }
    s.push_str("\nfn __bundle(lang: Lang) -> &'static __Bundle {\n    match lang {\n");
    for (code, variant) in LOCALES {
        let _ = writeln!(
            s,
            "        Lang::{variant} => {{\n            \
             static CELL: ::std::sync::OnceLock<__Bundle> = ::std::sync::OnceLock::new();\n            \
             CELL.get_or_init(|| __build({}, __SRC_{}))\n        }}",
            lit(code),
            variant.to_uppercase()
        );
    }
    s.push_str(
        "    }\n}\n\n\
         fn __build(tag: &str, src: &'static str) -> __Bundle {\n    \
         let res = ::fluent_bundle::FluentResource::try_new(src.to_owned())\n        \
         .expect(\"build.rs already parsed this\");\n    \
         let langid: ::unic_langid::LanguageIdentifier = tag.parse().expect(\"valid langid\");\n    \
         let mut b = __Bundle::new_concurrent(vec![langid]);\n    \
         // CLAUDE.md gotcha: Fluent wraps every placeable in U+2068/U+2069 by\n    \
         // default. They render as phantom width in the mono statusline and break\n    \
         // string comparison in tests. THIS IS THE ONLY CONSTRUCTOR — anything\n    \
         // that builds its own bundle reintroduces the bug.\n    \
         b.set_use_isolating(false);\n    \
         b.add_resource(res).expect(\"build.rs already checked for duplicates\");\n    \
         b\n}\n\n\
         fn __select(lang: Lang, id: &str, args: ::fluent_bundle::FluentArgs) -> String {\n    \
         let b = __bundle(lang);\n    \
         let msg = b.get_message(id).expect(\"generated ids always exist\");\n    \
         let pat = msg.value().expect(\"build.rs rejects value-less ids\");\n    \
         let mut errs = Vec::new();\n    \
         let out = b.format_pattern(pat, Some(&args), &mut errs).into_owned();\n    \
         debug_assert!(errs.is_empty(), \"{id}: {errs:?}\");\n    \
         out\n}\n",
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn mangle(id: &str) -> String {
    id.chars()
        .map(|c| if c == '-' || c == '.' { '_' } else { c })
        .collect()
}

/// `$n` and `$count` are the plural counters and arrive as integers; everything
/// else is already-rendered text. Stated as a convention in design/spec.md so
/// that a catalog author never has to think about Rust types.
fn is_count(var: &str) -> bool {
    matches!(var, "n" | "count")
}

fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let head = &src[..offset.min(src.len())];
    let line = head.matches('\n').count() + 1;
    let col = head
        .rsplit('\n')
        .next()
        .map_or(1, |l| l.chars().count() + 1);
    (line, col)
}

fn fail(msg: &str) -> ! {
    panic!("\n\n  ✗ {msg}\n");
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];
