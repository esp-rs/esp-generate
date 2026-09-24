//! The file-directive processor — a thin shell around [somni-template].
//!
//! The template *language* (`if`/`else`/`endif`, `for`, `{{ interpolation }}`,
//! the expression grammar) belongs to somni-template and is versioned by that
//! dependency. This module owns only the **facts** it evaluates against.
//!
//! It does **not** decide whether a file is emitted or what it is called: those
//! are rules in the template's `metadata.toml`, reaching the SDK through
//! [`Renderer::evaluate`] and [`Renderer::output_path`], so a manifest condition
//! and the same condition in a file body always agree.
//!
//! ## Syntax
//!
//! Directives are comment-shaped, so a template file stays valid source in its
//! own language. The marker is the file's comment prefix plus `%`:
//!
//! ```text
//! // an ordinary comment, emitted verbatim
//! //%if option("wifi")
//! //+let wifi = true;              // `//+` is stripped: emits `let wifi = true;`
//! //%endif
//! let chip = "{{ chip.name }}";
//! ```
//!
//! The `%` is load-bearing: with a bare `//` marker the engine parses *every*
//! comment as a directive.
//!
//! `//+` (`#+`, `--+`) is somni-template's `text_prefix`, which drops the
//! leading whitespace *along with* the prefix. Indentation that must survive
//! goes after the marker: `    //+    let p = init();` emits
//! `    let p = init();`.
//!
//! ## The facts
//!
//! Conditions and interpolations evaluate against one set of registrations,
//! built from [`Facts`] and the selected option names:
//!
//! - `option(name)` — is that option selected?
//! - `group_selected(group)` — does that selection group have a pick?
//! - one namespace per plugin the template declared, addressed `<name>.<field>`
//!   — the chip plugin contributes `chip`, so `chip.name`, `chip.rust_target`
//!   and a field per `esp-metadata` symbol. The SDK names none of them.
//!
//! [somni-template]: https://docs.rs/somni-template

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    rc::Rc,
    sync::Arc,
};

use indexmap::IndexMap;
use somni_expr::{Context, DynFunction};
use somni_template::{BlockStyle, Env, SomniStruct, Syntax, Template, TemplateTypes, TypedValue};

use crate::contract::is_reserved_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactValue {
    Str(String),
    Int(u64),
    Bool(bool),
}

impl std::fmt::Display for FactValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FactValue::Str(s) => f.write_str(s),
            FactValue::Int(i) => write!(f, "{i}"),
            FactValue::Bool(b) => write!(f, "{b}"),
        }
    }
}

impl From<bool> for FactValue {
    fn from(value: bool) -> Self {
        FactValue::Bool(value)
    }
}

impl From<String> for FactValue {
    fn from(value: String) -> Self {
        FactValue::Str(value)
    }
}

impl From<&str> for FactValue {
    fn from(value: &str) -> Self {
        FactValue::Str(value.to_string())
    }
}

impl From<u64> for FactValue {
    fn from(value: u64) -> Self {
        FactValue::Int(value)
    }
}

impl From<u32> for FactValue {
    fn from(value: u32) -> Self {
        FactValue::Int(value as u64)
    }
}

impl From<usize> for FactValue {
    fn from(value: usize) -> Self {
        FactValue::Int(value as u64)
    }
}

/// The valid names each string-argument predicate accepts.
///
/// A name outside its vocabulary is a hard [`ProcessError`], not a silent
/// `false`: a typo and a genuinely unselected option are otherwise
/// indistinguishable, and both quietly disable the block they guard.
///
/// `None` means the host supplied no vocabulary and the check is off.
/// `Some(empty)` is a real vocabulary that happens to be empty — a template
/// declaring no selection groups still gets `group_selected` checked.
#[derive(Debug, Default, Clone)]
pub struct Vocabulary {
    /// Every option name the template declares. Backs `option`.
    pub options: Option<HashSet<String>>,
    /// Every selection-group name the template declares. Backs `group_selected`.
    pub groups: Option<HashSet<String>>,
}

/// A namespace of fields a plugin contributes, addressed as `<name>.<field>`.
///
/// The plugin names and versions them, not the SDK. Behind an [`Arc`], so this
/// and [`Facts`] are O(1) to clone.
#[derive(Debug, Default, Clone)]
pub struct StructFacts {
    fields: Arc<IndexMap<Box<str>, FactValue>>,
}

impl StructFacts {
    pub fn new(fields: impl IntoIterator<Item = (Box<str>, FactValue)>) -> Self {
        StructFacts {
            fields: Arc::new(fields.into_iter().collect()),
        }
    }

    /// Every field a template can reference under this namespace.
    pub fn fields(&self) -> &IndexMap<Box<str>, FactValue> {
        &self.fields
    }

    fn to_struct(&self, name: &str) -> SomniStruct<TemplateTypes> {
        let fields = self
            .fields
            .iter()
            .map(|(field, value)| (field.clone(), typed(value)))
            .collect();
        SomniStruct::new(name.into(), fields)
    }
}

/// Everything the directive engine knows that isn't a user selection — the
/// single conduit from the host to the SDK.
#[derive(Debug, Default, Clone)]
pub struct Facts {
    /// Plugin-contributed namespaces, keyed by the name a template writes.
    /// One no plugin supplied is absent, so referencing it is an error.
    pub structs: IndexMap<String, StructFacts>,
    /// Known-name vocabularies backing the unknown-name hard error.
    pub vocabulary: Vocabulary,
    /// Substitution values: spliced into an output path, and exposed to
    /// expressions when the name is an identifier.
    pub values: HashMap<String, FactValue>,
}

impl Facts {
    /// One field of a plugin namespace, e.g. `chip`.`rust_target`. `None` when
    /// no resolved plugin supplied that namespace or that field.
    pub fn field(&self, namespace: &str, field: &str) -> Option<&FactValue> {
        self.structs.get(namespace)?.fields().get(field)
    }

    /// Insert a value, first writer wins. Binary facts go in before
    /// template-scoped `sets`, so a template can't shadow them.
    pub fn set_value(&mut self, key: impl Into<String>, value: impl Into<FactValue>) {
        self.values
            .entry(key.into())
            .or_insert_with(|| value.into());
    }
}

/// Whether `name` is writable as a somni identifier, and so referenceable from
/// an expression. Dashed names (`coding-agent-guidance-file`) are not.
fn is_somni_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A processing failure. Malformed directives, bad conditions, and
/// unresolvable includes are hard errors rather than panics or silent no-ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessError {
    pub line: Option<NonZeroUsize>,
    pub message: String,
}

impl ProcessError {
    /// A failure at a known line.
    fn at(line: NonZeroUsize, message: String) -> Self {
        ProcessError {
            line: Some(line),
            message,
        }
    }
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ProcessError {}

/// The comment convention a template file is written in. Fixes the directive
/// marker (`<base>%`) and the text-line prefix (`<base>+`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommentStyle {
    base: &'static str,
}

impl CommentStyle {
    /// Every convention the SDK understands, longest base first.
    const ALL: &'static [CommentStyle] = &[
        CommentStyle { base: "//" },
        CommentStyle { base: "--" },
        CommentStyle { base: "#" },
    ];

    /// Infer the convention from the first marker in `source`, whether a
    /// directive (`//%`) or a text-prefix line (`//+`).
    fn infer(source: &str) -> CommentStyle {
        source
            .lines()
            .find_map(|line| {
                let trimmed = line.trim_start();
                CommentStyle::ALL.iter().copied().find(|style| {
                    trimmed.starts_with(&style.marker())
                        || trimmed.starts_with(&style.text_prefix())
                })
            })
            .unwrap_or(CommentStyle { base: "#" })
    }

    /// The directive marker, e.g. `//%`.
    fn marker(&self) -> String {
        format!("{}%", self.base)
    }

    /// The text-line prefix, e.g. `//+`.
    fn text_prefix(&self) -> String {
        format!("{}+", self.base)
    }

    fn syntax(&self) -> Syntax {
        let mut syntax = Syntax::lines();
        syntax.block = BlockStyle::Line {
            prefix: self.marker(),
        };
        syntax.text_prefix = Some(self.text_prefix());
        syntax
    }
}

/// Everything the registered predicates need. [`Env::function`] requires
/// `'static` closures, hence the `Rc` rather than borrowing.
struct Shared {
    selected: Vec<String>,
    selected_groups: Vec<String>,
    values: HashMap<String, FactValue>,
    vocabulary: Vocabulary,
    /// Converted once from [`Facts::structs`]; refcounted, so registering them
    /// per file is a clone.
    structs: Vec<(String, SomniStruct<TemplateTypes>)>,
    /// The first unknown name seen while evaluating.
    ///
    /// somni predicates return a plain `bool` with no error channel, so a
    /// vocabulary miss is recorded here and promoted to a [`ProcessError`] once
    /// rendering returns. Only the first: short-circuiting means later names
    /// may never have been reached.
    unknown: RefCell<Option<String>>,
    /// Which registered predicates were called. Only ever as complete as the
    /// selections rendered.
    used: RefCell<Vec<&'static str>>,
}

impl Shared {
    fn note_unknown(&self, reason: String) {
        let mut slot = self.unknown.borrow_mut();
        if slot.is_none() {
            *slot = Some(reason);
        }
    }

    fn note_used(&self, name: &'static str) {
        let mut used = self.used.borrow_mut();
        if !used.contains(&name) {
            used.push(name);
        }
    }
}

/// Where fact registrations go: a template [`Env`], or a bare expression
/// [`Context`].
trait FactSink<'a> {
    fn fact(&mut self, name: &'a str, value: TypedValue<TemplateTypes>);
    fn func<F, A>(&mut self, name: &'a str, f: F)
    where
        F: DynFunction<A, TemplateTypes> + 'static;
}

impl<'a> FactSink<'a> for Env {
    fn fact(&mut self, name: &'a str, value: TypedValue<TemplateTypes>) {
        self.value(name, value);
    }
    fn func<F, A>(&mut self, name: &'a str, f: F)
    where
        F: DynFunction<A, TemplateTypes> + 'static,
    {
        self.function(name, f);
    }
}

impl<'a> FactSink<'a> for Context<'a, TemplateTypes> {
    fn fact(&mut self, name: &'a str, value: TypedValue<TemplateTypes>) {
        self.add_variable(name, value);
    }
    fn func<F, A>(&mut self, name: &'a str, f: F)
    where
        F: DynFunction<A, TemplateTypes> + 'static,
    {
        self.add_function(name, f);
    }
}

/// Register every fact onto `sink` — the single source of truth for the fact
/// API.
///
/// The option and group namespaces are **disjoint**, so a name that is both
/// can't be confused.
fn register_facts<'a>(sink: &mut impl FactSink<'a>, shared: &'a Rc<Shared>) {
    // First, so the reserved registrations below win.
    for (name, value) in &shared.values {
        if !is_somni_identifier(name) || is_reserved_name(name) {
            continue;
        }
        sink.fact(name.as_str(), typed(value));
    }

    let ctx = shared.clone();
    sink.func("option", move |name: &str| {
        ctx.note_used("option");
        if let Some(vocab) = &ctx.vocabulary.options
            && !vocab.contains(name)
        {
            ctx.note_unknown(format!(
                "unknown option `{name}` — the template declares no such option"
            ));
            return false;
        }
        ctx.selected.iter().any(|c| c == name)
    });

    let ctx = shared.clone();
    sink.func("group_selected", move |group: &str| {
        ctx.note_used("group_selected");
        if let Some(vocab) = &ctx.vocabulary.groups
            && !vocab.contains(group)
        {
            ctx.note_unknown(format!(
                "unknown selection group `{group}` — the template declares no such group"
            ));
            return false;
        }
        ctx.selected_groups.iter().any(|g| g == group)
    });

    // Last, so a namespace wins over a template value of the same name. These
    // belong to plugins, so they are not reserved names — order protects them.
    for (name, value) in &shared.structs {
        sink.fact(name.as_str(), TypedValue::Struct(value.clone()));
    }
}

fn typed(value: &FactValue) -> TypedValue<TemplateTypes> {
    match value {
        FactValue::Str(s) => TypedValue::String(s.as_str().into()),
        FactValue::Int(i) => TypedValue::Int(*i),
        FactValue::Bool(b) => TypedValue::Bool(*b),
    }
}

/// Whether `path` is a contained relative path: not absolute and free of any
/// `..` or drive-letter component. Used to reject include and output paths
/// that would let generation write outside the target directory.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.starts_with(['/', '\\']) {
        return false; // absolute (Unix or Windows)
    }
    if path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path.as_bytes().get(1) == Some(&b':')
    {
        return false; // drive-relative, e.g. `C:foo`
    }
    path.split(['/', '\\']).all(|part| part != "..")
}

/// The line a byte offset falls on, counting from 1.
fn line_of(source: &str, offset: usize) -> NonZeroUsize {
    let end = offset.min(source.len());
    let preceding = source
        .get(..end)
        .map_or(0, |head| head.matches('\n').count());
    // Line one, plus one per newline behind the offset: non-zero by
    // construction, so there is no fallible conversion to fudge.
    NonZeroUsize::MIN.saturating_add(preceding)
}

/// Resolves an `include` path to that template file's raw contents.
///
/// Paths are **template-root-relative**. Returning `Err` turns the include into
/// a compile error naming the path.
pub type IncludeLoader<'a> = &'a mut dyn FnMut(&str) -> Result<String, String>;

/// A prepared context: the selections and facts one generation run shares.
///
/// Built once and reused for every file and every manifest condition — passing
/// `&Facts` per call meant deep-cloning a few hundred fields each time.
pub struct Renderer {
    shared: Rc<Shared>,
}

impl Renderer {
    /// Prepare a context for one generation run.
    pub fn new(selected: &[String], selected_groups: &[String], facts: &Facts) -> Self {
        Renderer {
            shared: Rc::new(Shared {
                selected: selected.to_vec(),
                selected_groups: selected_groups.to_vec(),
                structs: facts
                    .structs
                    .iter()
                    .map(|(name, fields)| (name.clone(), fields.to_struct(name)))
                    .collect(),
                values: facts.values.clone(),
                vocabulary: facts.vocabulary.clone(),
                unknown: RefCell::new(None),
                used: RefCell::new(Vec::new()),
            }),
        }
    }

    /// The registered predicates evaluated so far, accumulated across calls.
    pub fn predicates_used(&self) -> Vec<&'static str> {
        self.shared.used.borrow().clone()
    }

    /// Render one template file.
    ///
    /// Whether the file is written, and under what name, is settled before this
    /// is called. `load` resolves `include` directives; an included file is a
    /// **partial**, inlined and sharing the caller's facts.
    pub fn render(&self, contents: &str, load: IncludeLoader<'_>) -> Result<String, ProcessError> {
        // A file that failed to *compile* never reached the point where the
        // slot is taken; the leftover would be blamed on this file.
        self.shared.unknown.borrow_mut().take();

        // U+FEFF isn't `char::is_whitespace`, so `trim` leaves a BOM attached
        // and demotes a first-line directive to text. Only the leading one is a
        // BOM; U+FEFF elsewhere is content.
        let body = contents.strip_prefix('\u{feff}').unwrap_or(contents);

        let style = CommentStyle::infer(body);
        let syntax = style.syntax();

        // Checked here too, so a filesystem-backed loader can't be walked out
        // of its root.
        let mut load_partial = |path: &str| -> Result<String, String> {
            if !is_safe_relative_path(path) {
                return Err(format!(
                    "`{path}` escapes the template root (absolute or `..` paths are not allowed)"
                ));
            }
            let raw = load(path)?;
            Ok(raw.strip_prefix('\u{feff}').unwrap_or(&raw).to_string())
        };

        let template = Template::compile_with(body, &syntax, &mut load_partial)
            .map_err(|e| template_error(body, e, "invalid template directive"))?;

        let mut env = Env::new();
        register_facts(&mut env, &self.shared);
        let rendered = template
            .render(env)
            .map_err(|e| template_error(body, e, "render failed"))?;

        // A miss outranks the value produced, so it is checked on success too.
        if let Some(reason) = self.shared.unknown.borrow_mut().take() {
            return Err(ProcessError::at(NonZeroUsize::MIN, reason));
        }

        Ok(rendered)
    }

    /// Evaluate a standalone boolean condition against the same facts a file
    /// body sees — the manifest's `when` entry point.
    ///
    /// `what` names the condition's source for the error message. There is no
    /// line to report: the condition isn't in a file.
    pub fn evaluate(&self, condition: &str, what: &str) -> Result<bool, ProcessError> {
        self.eval_condition(condition, what, None)
    }

    /// Render an output path — the manifest's `as` key. Ordinary template text,
    /// so `{{ name }}` interpolates as it does in a file body.
    pub fn output_path(&self, path: &str, what: &str) -> Result<String, ProcessError> {
        // Shared across every call on this context, so a miss left behind is
        // reported against whatever runs next.
        self.shared.unknown.borrow_mut().take();

        // A path is interpolated, never directive-parsed, so give the parser a
        // prefix no path can contain rather than an arbitrary comment style.
        let syntax = CommentStyle { base: "\0" }.syntax();

        let render = Template::compile(path, &syntax).and_then(|t| {
            let mut env = Env::new();
            register_facts(&mut env, &self.shared);
            t.render(env)
        });

        let fail = |reason: String| ProcessError {
            line: None,
            message: format!("invalid {what} `{path}`: {reason}"),
        };

        // A miss outranks the render result: rendering may have succeeded.
        if let Some(reason) = self.shared.unknown.borrow_mut().take() {
            return Err(fail(reason));
        }

        render.map_err(|e| fail(e.message.to_string()))
    }

    /// Evaluate one expression, with no template around it.
    fn eval_condition(
        &self,
        cond: &str,
        what: &str,
        line: Option<NonZeroUsize>,
    ) -> Result<bool, ProcessError> {
        self.shared.unknown.borrow_mut().take();

        let mut ctx = Context::<TemplateTypes>::new_with_types();
        register_facts(&mut ctx, &self.shared);

        let fail = |reason: String| ProcessError {
            line,
            message: format!("invalid {what} `{cond}`: {reason}"),
        };

        let result = ctx.evaluate::<bool>(cond);

        if let Some(reason) = self.shared.unknown.borrow_mut().take() {
            return Err(fail(reason));
        }

        match result {
            Ok(v) => Ok(v),
            Err(e) => Err(fail(e.into_inner().message.to_string())),
        }
    }
}

/// Map a [`somni_template::TemplateError`] back onto a source line.
fn template_error(body: &str, error: somni_template::TemplateError, what: &str) -> ProcessError {
    ProcessError::at(
        line_of(body, error.location.start),
        format!("{what}: {}", error.message),
    )
}

#[cfg(test)]
mod test {
    use super::*;

    /// A loader for the majority of tests, which use no `include` directives.
    fn no_partials() -> impl FnMut(&str) -> Result<String, String> {
        |path: &str| Err(format!("this test declares no partial `{path}`"))
    }

    /// A loader over a fixed `(path, contents)` table.
    fn partials(
        files: &'static [(&'static str, &'static str)],
    ) -> impl FnMut(&str) -> Result<String, String> {
        move |path: &str| {
            files
                .iter()
                .find_map(|(p, c)| (*p == path).then(|| c.to_string()))
                .ok_or_else(|| format!("no such partial: {path}"))
        }
    }

    /// Render with a selected-name list and no chip facts. Expects success.
    fn process(contents: &str, selected: &[&str]) -> String {
        let selected: Vec<String> = selected.iter().map(|s| s.to_string()).collect();
        Renderer::new(&selected, &[], &Facts::default())
            .render(contents, &mut no_partials())
            .expect("process_file should succeed")
    }

    /// Render with explicit option *and* group selections.
    fn process_with_groups(contents: &str, selected: &[&str], groups: &[&str]) -> String {
        let selected: Vec<String> = selected.iter().map(|s| s.to_string()).collect();
        let groups: Vec<String> = groups.iter().map(|s| s.to_string()).collect();
        Renderer::new(&selected, &groups, &Facts::default())
            .render(contents, &mut no_partials())
            .expect("process_file should succeed")
    }

    /// Render with facts and no selections.
    fn process_facts(contents: &str, facts: &Facts) -> String {
        Renderer::new(&[], &[], facts)
            .render(contents, &mut no_partials())
            .expect("process_file should succeed")
    }

    /// Returns the `ProcessError` or panics if the call unexpectedly succeeded.
    fn process_err(contents: &str, facts: &Facts) -> ProcessError {
        Renderer::new(&[], &[], facts)
            .render(contents, &mut no_partials())
            .expect_err("expected a ProcessError")
    }

    const NESTED: &str = "\
#%if option(\"opt1\")
opt1
#%if option(\"opt2\")
opt2
#%else
!opt2
#%endif
#%else
!opt1
#%endif
";

    #[test]
    fn nested_if_else_takes_the_inner_then_branch() {
        assert_eq!(process(NESTED, &["opt1", "opt2"]), "opt1\nopt2\n");
    }

    #[test]
    fn nested_if_else_takes_the_outer_else_branch() {
        assert_eq!(process(NESTED, &[]), "!opt1\n");
    }

    #[test]
    fn nested_if_else_takes_the_inner_else_branch() {
        assert_eq!(process(NESTED, &["opt1"]), "opt1\n!opt2\n");
    }

    #[test]
    fn else_if_chains_pick_the_first_true_arm() {
        let src = "\
#%if option(\"a\")
a
#%else if option(\"b\")
b
#%else if option(\"c\")
c
#%else
none
#%endif
";
        assert_eq!(process(src, &["a", "b"]), "a\n");
        assert_eq!(process(src, &["b", "c"]), "b\n");
        assert_eq!(process(src, &["c"]), "c\n");
        assert_eq!(process(src, &[]), "none\n");
    }

    #[test]
    fn indented_directives_are_recognized() {
        let src = "deps = [\n    #%if option(\"a\")\n    \"a\",\n    #%endif\n]\n";
        assert_eq!(process(src, &["a"]), "deps = [\n    \"a\",\n]\n");
        assert_eq!(process(src, &[]), "deps = [\n]\n");
    }

    #[test]
    fn ordinary_comments_are_not_directives() {
        // The whole reason the marker carries a `%`: a bare `//` prefix would
        // make the engine try to parse every comment as a directive.
        let src = "// just a comment\n//%if option(\"a\")\n//+let x = 1;\n//%endif\n";
        assert_eq!(process(src, &["a"]), "// just a comment\nlet x = 1;\n");
        assert_eq!(process(src, &[]), "// just a comment\n");
    }

    #[test]
    fn text_prefix_lines_are_emitted_uncommented() {
        assert_eq!(process("//+let x = 1;\n", &[]), "let x = 1;\n");
        assert_eq!(process("#+key = 1\n", &[]), "key = 1\n");
        assert_eq!(process("--+local x = 1\n", &[]), "local x = 1\n");
    }

    #[test]
    fn text_prefix_drops_the_indentation_before_the_marker() {
        // `text_prefix` removes the leading whitespace with the prefix, so
        // surviving indentation goes *after* the marker. Pinned because the
        // difference is invisible until you diff the output.
        assert_eq!(
            process("fn main() {\n    //+    let p = init();\n}\n", &[]),
            "fn main() {\n    let p = init();\n}\n"
        );
        assert_eq!(
            process("fn main() {\n    //+let p = init();\n}\n", &[]),
            "fn main() {\nlet p = init();\n}\n",
            "indentation before the marker is not preserved"
        );
    }

    #[test]
    fn text_prefix_is_only_stripped_at_line_start() {
        assert_eq!(
            process("let s = \"a //+ b\";\n", &[]),
            "let s = \"a //+ b\";\n"
        );
    }

    #[test]
    fn interpolation_reads_facts_values() {
        let mut facts = Facts::default();
        facts.set_value("project_name", "my-app");
        assert_eq!(
            process_facts("name = \"{{ project_name }}\"\n", &facts),
            "name = \"my-app\"\n"
        );
    }

    /// Rendering no longer decides whether or where a file lands — that is the
    /// manifest's job, so a head directive is just text now.
    #[test]
    fn the_renderer_no_longer_owns_the_file_lifecycle() {
        let out = process_facts("plain content\n", &Facts::default());
        assert_eq!(out, "plain content\n");
    }

    /// The variant-pair pattern: one emitted file picks one of two partials,
    /// so the choice is structurally exclusive instead of relying on two
    /// conditions staying exact negations of each other.
    #[test]
    fn a_conditional_include_picks_one_partial() {
        const FILES: &[(&str, &str)] = &[
            ("src/bin/main_async.rs", "async on {{ chip.name }}\n"),
            ("src/bin/main_blocking.rs", "blocking\n"),
        ];
        let src = "\
//%if option(\"embassy\")
//%include \"src/bin/main_async.rs\"
//%else
//%include \"src/bin/main_blocking.rs\"
//%endif
";
        let facts = facts_with_chip();

        let render = |selected: &[&str]| {
            let selected: Vec<String> = selected.iter().map(|s| s.to_string()).collect();
            Renderer::new(&selected, &[], &facts)
                .render(src, &mut partials(FILES))
                .unwrap()
        };

        assert_eq!(render(&["embassy"]), "async on esp32s3\n");
        assert_eq!(render(&[]), "blocking\n");
    }

    #[test]
    fn an_include_cannot_escape_the_template_root() {
        let err = Renderer::new(&[], &[], &Facts::default())
            .render("//%include \"../../etc/passwd\"\n", &mut partials(&[]))
            .expect_err("escaping include must be rejected");
        assert!(err.message.contains("escapes the template root"), "{err}");
    }

    #[test]
    fn a_missing_partial_is_a_hard_error() {
        let err = Renderer::new(&[], &[], &Facts::default())
            .render("//%include \"nope.rs\"\n", &mut partials(&[]))
            .expect_err("a missing partial must not render as empty");
        assert!(err.message.contains("nope.rs"), "{err}");
    }

    /// A file whose *content* legitimately contains `{{ … }}` — a GitHub
    /// Actions workflow — declares its own delimiters in frontmatter. Without
    /// this the bundled `rust_ci.yml` renders `${{ secrets.GITHUB_TOKEN }}` as
    /// an interpolation and fails with "Variable secrets was not found".
    #[test]
    fn frontmatter_can_move_the_interpolation_delimiters() {
        let mut facts = facts_with_chip();
        facts.set_value("project_name", "my-app");

        let src = "\
---
expr: <% %>
---
name: <% project_name %>
run: cargo ${{ matrix.action.command }}
";
        let out = Renderer::new(&[], &[], &facts)
            .render(src, &mut no_partials())
            .unwrap();

        // Frontmatter itself is consumed, ours is substituted, GitHub's is not.
        assert_eq!(
            out,
            "name: my-app\nrun: cargo ${{ matrix.action.command }}\n"
        );
    }

    #[test]
    fn a_manifest_condition_sees_the_same_facts_as_a_template_body() {
        let facts = facts_with_chip();
        let selected = vec!["wokwi".to_string()];
        let groups = vec!["chip".to_string()];

        let renderer = Renderer::new(&selected, &groups, &facts);
        let eval = |cond: &str| renderer.evaluate(cond, "`when` condition").unwrap();

        assert!(eval("option(\"wokwi\")"));
        assert!(!eval("option(\"embassy\")"));
        assert!(eval("group_selected(\"chip\")"));
        assert!(eval("chip.xtensa"));
        assert!(!eval("chip.riscv"));
        assert!(eval("option(\"wokwi\") && !chip.riscv"));
    }

    #[test]
    fn a_bad_manifest_condition_names_its_source() {
        let facts = facts_with_chip();
        let renderer = Renderer::new(&[], &[], &facts);
        let err = renderer
            .evaluate("chip.soc_has_wfi", "`emit.when` condition for `wokwi.toml`")
            .expect_err("a mistyped field must not be silently false");

        assert_eq!(
            err.message,
            "invalid `emit.when` condition for `wokwi.toml` `chip.soc_has_wfi`: \
             Struct `chip` has no field `soc_has_wfi`"
        );
    }

    /// A manifest condition is not *in* a template file, so there is no line to
    /// report — and `line 0:` would be worse than saying nothing.
    #[test]
    fn a_manifest_condition_error_carries_no_line() {
        let facts = facts_with_chip();
        let renderer = Renderer::new(&[], &[], &facts);
        let err = renderer
            .evaluate("chip.nope", "`emit.when` condition")
            .unwrap_err();

        assert_eq!(err.line, None);
        assert!(
            !err.to_string().starts_with("line "),
            "rendered as {:?}",
            err.to_string()
        );

        let in_file = process_err("#%if nope(\nx\n#%endif\n", &facts);
        assert!(in_file.line.is_some());
        assert!(in_file.to_string().starts_with("line "));
    }

    /// The context is reused across files, so the out-of-band unknown-name slot
    /// must not carry a miss from one file into the next. A file that fails to
    /// *compile* never reaches the point where the slot is taken.
    #[test]
    fn an_unknown_name_does_not_leak_between_files() {
        let facts = facts_with_vocabulary();
        let renderer = Renderer::new(&[], &[], &facts);

        // Records an unknown option, then fails to compile before taking it.
        let _ = renderer.render(
            "#%if option(\"wifii\")\nx\n#%endif\n#%if\n",
            &mut no_partials(),
        );

        let out = renderer
            .render("clean\n", &mut no_partials())
            .expect("a stale unknown name leaked into the next file");
        assert_eq!(out, "clean\n");
    }

    /// Every entry point clears the shared slot on the way in, so a miss one
    /// left behind is never blamed on the next caller.
    #[test]
    fn a_miss_never_leaks_into_a_later_condition() {
        let facts = facts_with_vocabulary();
        let renderer = Renderer::new(&[], &[], &facts);

        // Records the miss, then fails to render before the slot is drained.
        renderer
            .render(
                "#%if option(\"wifii\")\nx\n#%endif\n{{ 1 }}\n",
                &mut no_partials(),
            )
            .expect_err("an int cannot be interpolated bare");

        assert!(
            renderer
                .evaluate("option(\"alloc\")", "`emit.when` condition")
                .is_ok(),
            "a stale miss was blamed on the next condition"
        );
    }

    /// Render an output path, expecting success.
    fn out_path(facts: &Facts, path: &str) -> String {
        Renderer::new(&[], &[], facts)
            .output_path(path, "`emit.as` path")
            .expect("output path should render")
    }

    #[test]
    fn an_output_path_is_never_directive_parsed() {
        let mut facts = Facts::default();
        facts.set_value("name", "demo");

        for prefix in ["#", "//", "--", "%"] {
            let path = format!("{prefix}%if/{{{{ name }}}}.rs");
            assert_eq!(out_path(&facts, &path), format!("{prefix}%if/demo.rs"));
        }
    }

    #[test]
    fn an_output_path_interpolates_from_the_facts() {
        let mut facts = Facts::default();
        facts.set_value("coding_agent_guidance_file", "CLAUDE.md");
        facts.set_value("chip_name", "esp32c6");

        assert_eq!(
            out_path(&facts, "{{ coding_agent_guidance_file }}"),
            "CLAUDE.md"
        );
        assert_eq!(out_path(&facts, "src/{{ chip_name }}.rs"), "src/esp32c6.rs");
        assert_eq!(out_path(&facts, "wokwi.toml"), "wokwi.toml");
    }

    /// An unknown name in a path is an error, not a file literally called
    /// `{{ nope }}`. A file *body* keeps the override-or-default idiom; a path
    /// has no use for it.
    #[test]
    fn a_vocabulary_miss_in_an_output_path_is_reported_and_does_not_leak() {
        let facts = facts_with_vocabulary();
        let renderer = Renderer::new(&[], &[], &facts);

        let err = renderer
            .output_path("{{ str(option(\"wifii\")) }}.rs", "`emit.as` path")
            .expect_err("a misspelled option must not render as a filename");
        assert!(err.message.contains("unknown option"), "{err}");
        assert!(err.message.contains("wifii"), "{err}");

        assert!(
            renderer
                .evaluate("option(\"alloc\")", "`emit.when` condition")
                .is_ok(),
            "a miss leaked out of `output_path` and was blamed on the next call"
        );
    }

    #[test]
    fn an_unknown_name_in_an_output_path_is_an_error() {
        let facts = Facts::default();
        let err = Renderer::new(&[], &[], &facts)
            .output_path("src/{{ nope }}.rs", "`emit.as` path for `x`")
            .expect_err("an unresolved name must not reach the filesystem");

        assert!(err.message.contains("nope"), "{err}");
        assert!(err.message.contains("`emit.as` path for `x`"), "{err}");
        assert_eq!(err.line, None);
    }

    #[test]
    fn an_interpolated_path_cannot_smuggle_an_escape() {
        // `output_path` substitutes; the caller checks. Pinned together so the
        // pairing isn't lost.
        let mut facts = Facts::default();
        facts.set_value("evil", "../../etc/passwd");

        let path = out_path(&facts, "{{ evil }}");
        assert_eq!(path, "../../etc/passwd");
        assert!(
            !is_safe_relative_path(&path),
            "an escaping expansion must fail the path check"
        );
    }

    #[test]
    fn a_substituted_value_is_not_rescanned() {
        // One pass, so braces in a value are output, not re-expanded.
        let mut facts = Facts::default();
        facts.set_value("outer", "{{ inner }}");
        facts.set_value("inner", "leaf");

        assert_eq!(out_path(&facts, "src/{{ outer }}.rs"), "src/{{ inner }}.rs");
    }

    /// A `chip` namespace covering both capabilities, only one of which it has.
    fn facts_with_chip() -> Facts {
        Facts {
            structs: IndexMap::from([(
                "chip".to_string(),
                StructFacts::new([
                    ("name".into(), FactValue::Str("esp32s3".into())),
                    (
                        "rust_target".into(),
                        FactValue::Str("xtensa-esp32s3-none-elf".into()),
                    ),
                    ("dram2_uninit_size".into(), FactValue::Int(65536)),
                    ("xtensa".into(), FactValue::Bool(true)),
                    ("riscv".into(), FactValue::Bool(false)),
                    ("soc_has_wifi".into(), FactValue::Bool(true)),
                    ("soc_has_bt".into(), FactValue::Bool(false)),
                    ("bt_controller".into(), FactValue::Str(String::new())),
                ]),
            )]),
            ..Default::default()
        }
    }

    /// The mechanism a plugin relies on to put its own named fields beyond
    /// reach of the data it wraps: it appends them, and the last write wins.
    #[test]
    fn a_later_field_replaces_an_earlier_one_of_the_same_name() {
        let fields = StructFacts::new([
            ("rust_target".into(), FactValue::Str("from-metadata".into())),
            ("rust_target".into(), FactValue::Str("from-plugin".into())),
        ]);
        assert_eq!(
            fields.fields().get("rust_target"),
            Some(&FactValue::Str("from-plugin".into()))
        );
    }

    /// A namespace no plugin supplied does not exist, so referencing it is an
    /// error rather than a silent false.
    #[test]
    fn an_unsupplied_namespace_is_an_error() {
        let err = process_err("#%if board.has_led\nx\n#%endif\n", &Facts::default());
        assert!(err.message.contains("board"), "{err}");
    }

    #[test]
    fn chip_fields_drive_conditions() {
        let out = process_facts(
            "\
#%if chip.soc_has_wifi
has-wifi
#%endif
#%if chip.xtensa
xtensa
#%endif
#%if chip.riscv
riscv
#%endif
#%if chip.soc_has_bt
has-bt
#%endif
",
            &facts_with_chip(),
        );
        assert_eq!(out, "has-wifi\nxtensa\n");
    }

    #[test]
    fn chip_fields_interpolate_by_type() {
        let facts = facts_with_chip();
        assert_eq!(process_facts("{{ chip.name }}\n", &facts), "esp32s3\n");
        assert_eq!(
            process_facts("{{ str(chip.dram2_uninit_size) }}\n", &facts),
            "65536\n"
        );
    }

    /// Why the chip is a struct: a mistyped capability is an error naming the
    /// field, where a string predicate could only return `false` and silently
    /// disable the block. The counterpart matters as much — a capability the
    /// chip *lacks* must stay falsy, which is why every field is present.
    #[test]
    fn a_mistyped_capability_is_an_error_but_an_absent_one_is_false() {
        let facts = facts_with_chip();

        let out = process_facts("#%if chip.soc_has_bt\nyes\n#%else\nno\n#%endif\n", &facts);
        assert_eq!(out, "no\n", "an absent capability must stay falsy");

        let err = process_err("#%if chip.soc_has_wfi\nx\n#%endif\n", &facts);
        assert!(err.message.contains("soc_has_wfi"), "{err}");
        assert!(err.message.contains("no field"), "{err}");
    }

    #[test]
    fn a_template_value_cannot_shadow_the_chip_struct() {
        let mut facts = facts_with_chip();
        facts
            .values
            .insert("chip".to_string(), FactValue::Str("nonsense".into()));
        assert_eq!(process_facts("{{ chip.name }}\n", &facts), "esp32s3\n");
    }

    #[test]
    fn without_a_chip_the_namespace_does_not_exist() {
        let err = process_err("#%if chip.riscv\nx\n#%endif\n", &Facts::default());
        assert!(err.message.contains("chip"), "{err}");
    }

    #[test]
    fn group_selected_reads_selection_group_names() {
        let out = process_with_groups(
            "#%if group_selected(\"chip\")\nchip-picked\n#%endif\n",
            &["esp32c6"],
            &["chip"],
        );
        assert_eq!(out, "chip-picked\n");
    }

    #[test]
    fn option_and_group_namespaces_are_disjoint() {
        // The bundled template has a name that is BOTH a category and a
        // selection group (`coding-agent-guidance`); the two predicates must
        // not answer for each other.
        let out = process_with_groups(
            "\
#%if option(\"coding-agent-guidance\")
option-hit
#%endif
#%if group_selected(\"coding-agent-guidance\")
group-hit
#%endif
#%if option(\"claude\")
claude-hit
#%endif
#%if group_selected(\"claude\")
claude-group-hit
#%endif
",
            &["claude"],
            &["coding-agent-guidance"],
        );
        assert_eq!(out, "group-hit\nclaude-hit\n");
    }

    #[test]
    fn values_are_readable_in_conditions() {
        let mut facts = facts_with_chip();
        facts.set_value("dram2_uninit_size", 1024u64);

        let out = process_facts(
            "\
#%if chip.name == \"esp32s3\"
is-s3
#%endif
#%if chip.name == \"esp32\"
is-esp32
#%endif
#%if dram2_uninit_size > 0
has-dram2
#%endif
#%if dram2_uninit_size > 4096
big-dram2
#%endif
",
            &facts,
        );
        assert_eq!(out, "is-s3\nhas-dram2\n");
    }

    /// A value no host supplied is a hard error naming it, in a condition and
    /// in an interpolation alike.
    #[test]
    fn a_value_no_host_supplied_is_a_loud_error() {
        let err = process_err(
            "#%if project_name == \"x\"\ny\n#%endif\n",
            &Facts::default(),
        );
        assert!(err.message.contains("project_name"), "{err}");

        let err = process_err("name = \"{{ project_name }}\"\n", &Facts::default());
        assert!(err.message.contains("project_name"), "{err}");
    }

    #[test]
    fn a_binary_value_beats_a_later_template_value() {
        // What protects a host-supplied name from a `sets` key of the same
        // name: the host writes first and `set_value` keeps the first writer.
        let mut facts = Facts::default();
        facts.set_value("has_reserved_pins", true);
        facts.set_value("has_reserved_pins", false);

        let out = process_facts(
            "#%if has_reserved_pins\nbinary\n#%else\ntemplate\n#%endif\n",
            &facts,
        );
        assert_eq!(out, "binary\n");
    }

    #[test]
    fn somni_identifier_classifies() {
        assert!(is_somni_identifier("chip"));
        assert!(is_somni_identifier("_private"));
        assert!(is_somni_identifier("dram2_uninit_size"));
        assert!(!is_somni_identifier("coding-agent-guidance-file"));
        assert!(!is_somni_identifier("2fast"));
        assert!(!is_somni_identifier(""));
        assert!(!is_somni_identifier("has space"));
    }

    #[test]
    fn is_safe_relative_path_classifies() {
        assert!(is_safe_relative_path("src/main.rs"));
        assert!(is_safe_relative_path("./a/b.rs"));
        assert!(!is_safe_relative_path("/abs"));
        assert!(!is_safe_relative_path("../x"));
        assert!(!is_safe_relative_path("a/../../b"));
        // Windows drive-relative — Path treats as Normal on Unix, but the
        // resolution semantics are unsafe on Windows.
        assert!(!is_safe_relative_path("C:foo"));
        assert!(!is_safe_relative_path("C:\\foo"));
        assert!(!is_safe_relative_path("z:"));
    }

    /// Facts carrying the option/group vocabularies that back the unknown-name
    /// error for the two string-argument predicates. Chip capabilities need no
    /// vocabulary — they are struct fields, and somni reports an unknown one.
    fn facts_with_vocabulary() -> Facts {
        Facts {
            vocabulary: Vocabulary {
                options: Some(HashSet::from(["alloc".to_string(), "wifi".to_string()])),
                groups: Some(HashSet::from(["chip".to_string(), "flashing".to_string()])),
            },
            ..Default::default()
        }
    }

    #[test]
    fn unknown_option_and_group_names_are_hard_errors() {
        let facts = facts_with_vocabulary();

        let out = process_facts("#%if option(\"wifi\")\nyes\n#%else\nno\n#%endif\n", &facts);
        assert_eq!(out, "no\n");

        // A misspelling would otherwise silently disable the block it guards.
        let err = process_err("#%if option(\"wifii\")\nx\n#%endif\n", &facts);
        assert!(err.message.contains("unknown option"), "{err}");
        assert!(err.message.contains("wifii"), "{err}");

        let err = process_err("#%if group_selected(\"flashng\")\nx\n#%endif\n", &facts);
        assert!(err.message.contains("unknown selection group"), "{err}");

        let err = process_err("#%if option(\"flashing\")\nx\n#%endif\n", &facts);
        assert!(err.message.contains("unknown option"), "{err}");
    }

    #[test]
    fn unknown_names_are_caught_in_every_condition_directive() {
        let facts = facts_with_vocabulary();

        for template in [
            "#%if option(\"nope\")\nx\n#%endif\n",
            "#%if option(\"alloc\")\nx\n#%else if option(\"nope\")\ny\n#%endif\n",
        ] {
            let err = process_err(template, &facts);
            assert!(
                err.message.contains("unknown option"),
                "{template:?} -> {err}"
            );
        }
    }

    /// "No vocabulary supplied" and "supplied, and empty" are different: the
    /// second is a real answer. A template declaring no selection groups would
    /// otherwise get no `group_selected` checking at all — the exact case the
    /// check exists for.
    #[test]
    fn an_absent_vocabulary_is_not_an_empty_one() {
        let src = "#%if option(\"whatever\")\nyes\n#%else\nno\n#%endif\n";

        // Not supplied: permissive.
        assert_eq!(process_facts(src, &Facts::default()), "no\n");

        // Supplied and empty: no name can be valid.
        let declared_nothing = Facts {
            vocabulary: Vocabulary {
                options: Some(HashSet::new()),
                groups: Some(HashSet::new()),
            },
            ..Default::default()
        };
        let err = process_err(src, &declared_nothing);
        assert!(err.message.contains("unknown option"), "{err}");

        let err = process_err(
            "#%if group_selected(\"nope\")\nx\n#%endif\n",
            &declared_nothing,
        );
        assert!(err.message.contains("unknown selection group"), "{err}");
    }

    #[test]
    fn a_stale_unknown_name_does_not_leak_into_a_later_condition() {
        // A miss inside a *skipped* branch is never evaluated, so it must not
        // surface later and misattribute the error to an innocent line.
        let facts = facts_with_vocabulary();
        let out = Renderer::new(&["alloc".to_string()], &[], &facts).render("#%if option(\"alloc\")\nkept\n#%else\n#%if option(\"bogus\")\nx\n#%endif\n#%endif\n", &mut no_partials())
        .unwrap();
        assert_eq!(out, "kept\n");
    }

    #[test]
    fn non_ascii_text_survives_every_stage() {
        // Path interpolation is byte-oriented in places (`find('{')`,
        // `as_bytes()`), so multi-byte content must not be corrupted or panic a
        // slice on a non-char boundary.
        let mut facts = Facts::default();
        facts.set_value("chip_name", "esp32c6");
        facts.set_value("emoji", "🦀");

        let out = process_facts("let s = \"héllo → wörld 日本語 🦀\";\n", &facts);
        assert_eq!(out, "let s = \"héllo → wörld 日本語 🦀\";\n");

        assert_eq!(
            out_path(&facts, "src/日本{{ chip_name }}語/{{ emoji }}.rs"),
            "src/日本esp32c6語/🦀.rs"
        );

        let out = process(
            "#%if option(\"öpt\")\nhit\n#%else\nmiss\n#%endif\n",
            &["öpt"],
        );
        assert_eq!(out, "hit\n");
    }

    #[test]
    fn leading_byte_order_mark_does_not_hide_directives() {
        // U+FEFF is not `char::is_whitespace`, so `trim()` leaves a Windows
        // editor's BOM attached to the first directive, demoting it to text.

        let out = process("\u{feff}#%if option(\"x\")\nbody\n#%endif\n", &[]);
        assert_eq!(out, "", "BOM hid the `if`");

        // A BOM ahead of ordinary content is stripped, not emitted: it would
        // otherwise corrupt the first token of a generated Rust/TOML file.
        assert_eq!(process("\u{feff}fn main() {}\n", &[]), "fn main() {}\n");

        assert_eq!(process("a\u{feff}b\n", &[]), "a\u{feff}b\n");
    }

    #[test]
    fn bad_condition_is_an_error_not_a_panic() {
        let err = process_err(
            "#%if definitely_not_a_fact\nx\n#%endif\n",
            &Facts::default(),
        );
        assert!(err.message.contains("definitely_not_a_fact"), "{err}");
    }

    #[test]
    fn condition_errors_are_a_single_plain_line() {
        // somni's `Debug` rendering is an ANSI-coloured multi-line caret
        // diagram whose line numbers are relative to the expression, not the
        // file — it must not end up inside our `file:line` message.
        let err = process_err(
            "#%if definitely_not_a_fact\nx\n#%endif\n",
            &Facts::default(),
        );
        let rendered = err.to_string();
        assert!(
            !rendered.contains('\u{1b}'),
            "ANSI escape leaked: {rendered:?}"
        );
        assert!(!rendered.contains('\n'), "multi-line message: {rendered:?}");
    }

    #[test]
    fn endif_and_else_tolerate_a_trailing_label() {
        // The bundled `Cargo.toml` annotates which block is closing
        // (`#%endif wifi || ble-trouble`). The label is ignored, not emitted.
        let src = "#%if option(\"a\")\nx\n#%endif a || b\n";
        assert_eq!(process(src, &["a"]), "x\n");
        assert_eq!(process(src, &[]), "");

        // `else` is the exception: it has a meaningful continuation (`else
        // if`), so it parses strictly and a label is a hard error rather than
        // being read as a malformed `else if`.
        let err = process_err(
            "#%if option(\"a\")\nx\n#%else not-a\ny\n#%endif\n",
            &Facts::default(),
        );
        assert!(
            err.message.contains("after `else`"),
            "expected a clear diagnostic, got: {err}"
        );
        assert_eq!(err.line, NonZeroUsize::new(3));
    }

    #[test]
    fn unbalanced_directives_are_hard_errors_not_panics() {
        for bad in [
            "body\n#%endif\n",
            "#%else\nbody\n",
            "#%if option(\"x\")\nbody\n",
        ] {
            let err = process_err(bad, &Facts::default());
            assert!(err.line.is_some(), "{bad:?} -> {err}");
        }
    }

    #[test]
    fn int_values_interpolate_as_decimal_and_compare_as_numbers() {
        // Interpolation emits strings only, so an int needs `str()`. Pinned
        // because the bare form fails at *render* time, not compile time.
        let mut facts = Facts::default();
        facts.set_value("dram2_uninit_size", 32768u64);

        assert_eq!(
            process_facts("size: {{ str(dram2_uninit_size) }});\n", &facts),
            "size: 32768);\n"
        );
        assert_eq!(
            process_facts("#%if dram2_uninit_size > 4096\nbig\n#%endif\n", &facts),
            "big\n"
        );

        let err = process_err("size: {{ dram2_uninit_size }};\n", &facts);
        assert!(err.message.contains("to be &str"), "{err}");
    }

    #[test]
    fn a_dashed_value_name_is_unreachable() {
        // Not a somni identifier, so it is never registered. Paths render
        // through the same engine as bodies, so it is out of reach from both.
        let mut facts = Facts::default();
        facts.set_value("coding-agent-guidance-file", "CLAUDE.md");

        let err = process_err("#%if coding-agent-guidance-file\nx\n#%endif\n", &facts);
        assert!(!err.message.is_empty(), "{err}");

        Renderer::new(&[], &[], &facts)
            .output_path("{{ coding-agent-guidance-file }}", "`emit.as` path")
            .expect_err("a dashed name cannot be interpolated anywhere");
    }
}
