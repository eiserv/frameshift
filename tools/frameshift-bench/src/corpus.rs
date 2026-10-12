//! Deterministic generator of fictional JavaScript source files (`D-040`).
//!
//! Output depends only on the seed and [`GENERATOR_VERSION`]. Every decision
//! uses integer arithmetic on a SplitMix64 sequence, because floating-point
//! library functions may round differently across platforms. Each declared
//! name is unique within its file, so generated code has no redeclaration
//! errors, and each construct is emitted only where its goal and function
//! context permit it. All content is invented for Frameshift.

use frameshift_engine::SourceGoal;

/// Version of the generated output. Any change to the bytes produced for a
/// seed requires a new version and a new pinned fingerprint.
pub const GENERATOR_VERSION: u32 = 1;

/// Seed of the default corpus.
pub const DEFAULT_SEED: u64 = 0x00E1_0BE0_C0DE_5EED;

/// SplitMix64 (Steele, Lea, and Flood, 2014): fully specified by integer
/// operations and statistically adequate for shaping a corpus.
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Starts a sequence at `seed`.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Returns the next value of the sequence.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    /// Returns a value in `0..bound` by multiplication instead of modulo,
    /// which keeps the bias negligible for small bounds. `bound` must be
    /// positive.
    pub fn below(&mut self, bound: u64) -> u64 {
        ((u128::from(self.next_u64()) * u128::from(bound)) >> 64) as u64
    }

    fn index(&mut self, len: usize) -> usize {
        self.below(len as u64) as usize
    }

    fn percent(&mut self, chance: u64) -> bool {
        self.below(100) < chance
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.index(items.len())]
    }
}

/// One generated source file and the goal it must be parsed with.
pub struct GeneratedFile {
    /// Script for CommonJS-style files, Module otherwise.
    pub goal: SourceGoal,
    /// The complete source text.
    pub text: String,
}

/// A generated corpus in a fixed order.
pub struct Corpus {
    /// The files, in generation order.
    pub files: Vec<GeneratedFile>,
}

impl Corpus {
    /// Generates `count` files whose sizes follow [`mixed_file_size`]. About
    /// one file in ten is a CommonJS-style script, like configuration files
    /// next to application modules.
    pub fn mixed(seed: u64, count: usize) -> Self {
        let mut rng = Rng::new(seed);
        let files = (0..count)
            .map(|_| {
                let target = mixed_file_size(&mut rng);
                let goal = if rng.percent(10) {
                    SourceGoal::Script
                } else {
                    SourceGoal::Module
                };
                generate_file(rng.next_u64(), goal, target)
            })
            .collect();
        Self { files }
    }

    /// Returns the total source size in bytes.
    pub fn bytes(&self) -> usize {
        self.files.iter().map(|file| file.text.len()).sum()
    }

    /// Returns the number of files generated for the Script goal.
    pub fn scripts(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.goal == SourceGoal::Script)
            .count()
    }

    /// Returns a 64-bit FNV-1a hash over each file's goal, length, and bytes.
    /// It identifies a corpus across platforms; it is not a security hash.
    pub fn fingerprint(&self) -> u64 {
        const PRIME: u64 = 0x0000_0100_0000_01B3;
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for &byte in bytes {
                hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
            }
        };
        for file in &self.files {
            feed(match file.goal {
                SourceGoal::Script => b"S",
                SourceGoal::Module => b"M",
            });
            feed(&(file.text.len() as u64).to_le_bytes());
            feed(file.text.as_bytes());
        }
        hash
    }
}

/// Draws the target size of one file in the mixed corpus: piecewise
/// log-uniform from 1 KiB to 32 KiB, so every size class from a small
/// component to a large data module is equally frequent. The mean target is
/// about 9.5 KB; with the overshoot of the last item, 1,000 files hold about
/// 10 MB, close to the `analyze` budget scenario.
pub fn mixed_file_size(rng: &mut Rng) -> usize {
    let exponent = 10 + rng.below(5);
    let base = 1_u64 << exponent;
    (base + rng.below(base)) as usize
}

/// Generates one file of at least `target` bytes. It exceeds the target by
/// at most one top-level item.
pub fn generate_file(seed: u64, goal: SourceGoal, target: usize) -> GeneratedFile {
    let mut emitter = Emitter::new(seed, goal);
    emitter.file(target);
    GeneratedFile {
        goal,
        text: emitter.out,
    }
}

const NOUNS: [&str; 30] = [
    "post", "page", "route", "user", "item", "cart", "price", "title", "slug", "locale", "theme",
    "config", "entry", "result", "count", "label", "status", "author", "comment", "image", "link",
    "menu", "section", "token", "query", "filter", "order", "product", "review", "session",
];

const VERBS: [&str; 15] = [
    "load",
    "format",
    "build",
    "parse",
    "render",
    "create",
    "update",
    "resolve",
    "normalize",
    "collect",
    "select",
    "compute",
    "validate",
    "merge",
    "fetch",
];

const CLASS_SUFFIXES: [&str; 8] = [
    "Store",
    "Cache",
    "Client",
    "Registry",
    "Queue",
    "Router",
    "Formatter",
    "Builder",
];

const PROPERTIES: [&str; 22] = [
    "id",
    "name",
    "title",
    "slug",
    "href",
    "path",
    "items",
    "length",
    "value",
    "data",
    "status",
    "count",
    "label",
    "locale",
    "enabled",
    "children",
    "options",
    "createdAt",
    "tags",
    "meta",
    "default",
    "size",
];

const METHODS: [&str; 20] = [
    "map",
    "filter",
    "reduce",
    "find",
    "includes",
    "join",
    "split",
    "trim",
    "toLowerCase",
    "replace",
    "push",
    "slice",
    "get",
    "set",
    "has",
    "then",
    "toString",
    "format",
    "json",
    "forEach",
];

const GLOBALS: [&str; 15] = [
    "Math",
    "JSON",
    "Object",
    "Array",
    "Date",
    "Number",
    "String",
    "Promise",
    "console",
    "window",
    "document",
    "process",
    "globalThis",
    "Intl",
    "Symbol",
];

const CONSTRUCTORS: [&str; 8] = [
    "Map",
    "Set",
    "Date",
    "URL",
    "Error",
    "TypeError",
    "URLSearchParams",
    "AbortController",
];

const NUMBERS: [&str; 18] = [
    "0",
    "1",
    "2",
    "10",
    "42",
    "100",
    "1024",
    "3.14",
    "0.5",
    ".25",
    "1_000_000",
    "0xff",
    "0b1010",
    "0o755",
    "1e3",
    "2.5e-4",
    "9007199254740993n",
    "60_000",
];

// Escape spellings are doubled backslashes in Rust, so the generated text
// holds one backslash per escape.
const STRINGS: [&str; 20] = [
    "\"Hello, world\"",
    "'it\\'s ready'",
    "\"line\\nbreak\"",
    "\"Grüße aus München\"",
    "\"caf\\u00e9\"",
    "\"\\u{1F680} launched\"",
    "\"tab\\tseparated\"",
    "'say \"hi\"'",
    "\"/blog/archive\"",
    "\"\"",
    "\"en-US\"",
    "'de-DE'",
    "\"application/json\"",
    "\"data-state\"",
    "\"Read more\"",
    "'naïve café'",
    "\"\\x41\\x42\"",
    "\"C:\\\\temp\\\\cache\"",
    "\"✓ saved\"",
    "\"#2f6feb\"",
];

const REGEXPS: [&str; 10] = [
    r"/^[a-z0-9-]+$/i",
    r"/\s+/g",
    r"/(\d{4})-(\d{2})-(\d{2})/",
    r"/^https?:\/\//",
    r"/[.*+?^${}()|[\]\\]/g",
    r"/(?<year>\d{4})-(?<month>\d{2})/u",
    r"/\p{L}+/gu",
    r"/^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i",
    r"/[^\w\s-]/g",
    r"/(?:^|\s)tag-(\w+)/g",
];

const TEMPLATE_TEXTS: [&str; 10] = [
    "/blog/",
    " items",
    "Hello, ",
    " of ",
    "Total: ",
    "?page=",
    "&sort=",
    " \\u00b7 ",
    "\\n",
    "Price \\$",
];

const COMMENTS: [&str; 10] = [
    "Keeps the order stable for rendering.",
    "Fallback for missing translations.",
    "The API returns dates as ISO strings.",
    "Avoids a second pass over the list.",
    "Übersetzungen werden lazy geladen.",
    "Matches the slug format of the CMS.",
    "Cached per request; see the loader.",
    "Values are in cents to avoid rounding.",
    "Hidden entries stay in the sitemap.",
    "naïve check, sufficient for fixtures",
];

const PATHS: [&str; 10] = [
    "./utils.js",
    "../lib/format.js",
    "./components/card.js",
    "../data/site.js",
    "@fictional/ui",
    "fictional-icons",
    "./hooks/use-cart.js",
    "../config/routes.js",
    "node:path",
    "./constants.mjs",
];

// Indentation depth from which no further blocks or nested functions are
// generated. Every block indents, so this bounds the size of each item.
const MAX_NESTING: usize = 4;

const BINARY_OPERATORS: [&str; 17] = [
    "+", "-", "*", "/", "%", "===", "!==", "<", "<=", ">", ">=", "&&", "||", "??", "==", "|", "&",
];

// How a function declaration is exported.
#[derive(Clone, Copy)]
enum Export {
    No,
    Maybe,
    Default,
}

// Where the code being generated sits. Function boundaries replace it.
#[derive(Clone, Copy)]
struct Frame {
    // `return` is permitted.
    function: bool,
    is_async: bool,
    generator: bool,
    // `continue` is permitted.
    in_loop: bool,
    // `break` is permitted.
    breakable: bool,
    // Private names of the enclosing class may be referenced.
    in_class: bool,
    // `super.name` is permitted.
    super_property: bool,
}

impl Frame {
    const TOP: Self = Self {
        function: false,
        is_async: false,
        generator: false,
        in_loop: false,
        breakable: false,
        in_class: false,
        super_property: false,
    };

    fn function(self, is_async: bool, generator: bool) -> Self {
        Self {
            function: true,
            is_async,
            generator,
            in_loop: false,
            breakable: false,
            in_class: self.in_class,
            super_property: false,
        }
    }

    // Arrow functions keep `super` but never yield.
    fn arrow(self, is_async: bool) -> Self {
        Self {
            generator: false,
            ..self.function(is_async, false)
        }
        .with_super(self.super_property)
    }

    fn with_super(self, super_property: bool) -> Self {
        Self {
            super_property,
            ..self
        }
    }

    fn looping(self) -> Self {
        Self {
            in_loop: true,
            breakable: true,
            ..self
        }
    }
}

struct Emitter {
    rng: Rng,
    out: String,
    goal: SourceGoal,
    newline: &'static str,
    indent_unit: &'static str,
    semicolons: bool,
    indent: usize,
    next_id: u32,
    // Top-level functions and constants, for references and export lists.
    top_level: Vec<String>,
    // Top-level classes, for `extends`, `new`, and `instanceof`.
    classes: Vec<String>,
    // Locals in scope, truncated when a block or function ends.
    locals: Vec<String>,
    // `let` bindings in scope that may be reassigned.
    mutable: Vec<String>,
    // Private names of the class being generated.
    privates: Vec<String>,
    default_exported: bool,
}

impl Emitter {
    fn new(seed: u64, goal: SourceGoal) -> Self {
        let mut rng = Rng::new(seed);
        let newline = if rng.percent(10) { "\r\n" } else { "\n" };
        let indent_unit = rng.pick(&["  ", "  ", "  ", "    ", "\t"]);
        let semicolons = rng.percent(80);
        Self {
            rng,
            out: String::new(),
            goal,
            newline,
            indent_unit,
            semicolons,
            indent: 0,
            next_id: 0,
            top_level: Vec::new(),
            classes: Vec::new(),
            locals: Vec::new(),
            mutable: Vec::new(),
            privates: Vec::new(),
            default_exported: false,
        }
    }

    fn file(&mut self, target: usize) {
        match self.goal {
            SourceGoal::Module => self.imports(),
            SourceGoal::Script => self.script_prologue(),
        }
        while self.out.len() < target {
            self.top_level_item();
        }
        match self.goal {
            SourceGoal::Module => {
                if !self.default_exported && self.rng.percent(50) {
                    let value = self.object_literal(Frame::TOP, 2);
                    self.line(&format!("export default {value}{}", self.semi()));
                }
            }
            SourceGoal::Script => {
                let names = self.export_names();
                self.line(&format!("module.exports = {{ {names} }}{}", self.semi()));
            }
        }
    }

    fn fresh(&mut self, base: &str) -> String {
        self.next_id += 1;
        format!("{base}{}", self.next_id)
    }

    fn fresh_noun(&mut self) -> String {
        let noun = self.rng.pick(&NOUNS);
        self.fresh(noun)
    }

    fn fresh_function_name(&mut self) -> String {
        let verb = self.rng.pick(&VERBS);
        let noun = capitalize(self.rng.pick(&NOUNS));
        self.fresh(&format!("{verb}{noun}"))
    }

    fn semi(&self) -> &'static str {
        if self.semicolons { ";" } else { "" }
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str(self.indent_unit);
        }
        self.out.push_str(text);
        self.out.push_str(self.newline);
    }

    fn blank(&mut self) {
        self.out.push_str(self.newline);
    }

    fn comment(&mut self) {
        let text = self.rng.pick(&COMMENTS);
        match self.rng.below(3) {
            0 => self.line(&format!("/* {text} */")),
            _ => self.line(&format!("// {text}")),
        }
    }

    fn jsdoc(&mut self, parameters: &[String]) {
        let summary = self.rng.pick(&COMMENTS);
        self.line("/**");
        self.line(&format!(" * {summary}"));
        for parameter in parameters {
            let kind = self
                .rng
                .pick(&["string", "number", "object", "Array<string>"]);
            self.line(&format!(" * @param {{{kind}}} {parameter}"));
        }
        self.line(" */");
    }

    fn imports(&mut self) {
        for _ in 0..self.rng.below(5) {
            let path = self.rng.pick(&PATHS);
            let declaration = match self.rng.below(6) {
                0 => {
                    let name = self.fresh_noun();
                    self.top_level.push(name.clone());
                    format!("import {name} from \"{path}\"")
                }
                1 => {
                    let name = self.fresh("icons");
                    self.top_level.push(name.clone());
                    format!("import * as {name} from \"{path}\"")
                }
                2 => format!("import \"{path}\""),
                3 => {
                    let name = self.fresh("data");
                    self.top_level.push(name.clone());
                    format!("import {name} from \"{path}\" with {{ type: \"json\" }}")
                }
                _ => {
                    let mut specifiers = Vec::new();
                    for _ in 0..=self.rng.below(3) {
                        let name = self.fresh_function_name();
                        if self.rng.percent(30) {
                            let local = self.fresh_noun();
                            specifiers.push(format!("{name} as {local}"));
                            self.top_level.push(local);
                        } else {
                            specifiers.push(name.clone());
                            self.top_level.push(name);
                        }
                    }
                    format!("import {{ {} }} from \"{path}\"", specifiers.join(", "))
                }
            };
            self.line(&format!("{declaration}{}", self.semi()));
        }
        self.blank();
    }

    fn script_prologue(&mut self) {
        if self.rng.percent(5) {
            self.line("#!/usr/bin/env node");
        }
        if self.rng.percent(30) {
            self.line(&format!("\"use strict\"{}", self.semi()));
        }
        for _ in 0..self.rng.below(4) {
            let name = self.fresh_noun();
            let path = self.rng.pick(&PATHS);
            let kind = self.rng.pick(&["const", "const", "var"]);
            self.line(&format!(
                "{kind} {name} = require(\"{path}\"){}",
                self.semi()
            ));
            self.top_level.push(name);
        }
        self.blank();
    }

    fn export_names(&mut self) -> String {
        let mut names = Vec::new();
        for _ in 0..=self.rng.below(4) {
            match self.top_level.len() {
                0 => break,
                len => names.push(self.top_level[self.rng.index(len)].clone()),
            }
        }
        names.sort();
        names.dedup();
        names.join(", ")
    }

    fn export_prefix(&mut self, chance: u64) -> &'static str {
        if self.goal == SourceGoal::Module && self.rng.percent(chance) {
            "export "
        } else {
            ""
        }
    }

    fn top_level_item(&mut self) {
        if self.rng.percent(15) {
            self.comment();
        }
        match self.rng.below(20) {
            0..=4 => self.constant(),
            5..=10 => self.function_declaration(Frame::TOP, Export::Maybe, 2),
            11..=13 => self.class_declaration(),
            14 => self.destructuring(Frame::TOP, 2),
            15 if self.goal == SourceGoal::Module => self.export_list(),
            16 if self.goal == SourceGoal::Module && self.rng.percent(30) => {
                let name = self.fresh("settings");
                let call = self.call(Frame::TOP, 1);
                self.line(&format!("const {name} = await {call}{}", self.semi()));
                self.top_level.push(name);
            }
            17 if self.goal == SourceGoal::Module && !self.default_exported => {
                self.default_exported = true;
                self.function_declaration(Frame::TOP, Export::Default, 2);
            }
            _ => {
                let statement = self.expression_statement(Frame::TOP, 2);
                self.line(&statement);
            }
        }
        self.blank();
    }

    fn constant(&mut self) {
        let export = self.export_prefix(50);
        let name = if self.rng.percent(30) {
            let noun = self.rng.pick(&NOUNS).to_uppercase();
            self.fresh(&noun)
        } else {
            self.fresh_noun()
        };
        let value = self.expression(Frame::TOP, 3);
        self.line(&format!("{export}const {name} = {value}{}", self.semi()));
        self.top_level.push(name);
    }

    fn export_list(&mut self) {
        match self.rng.below(3) {
            0 => {
                let path = self.rng.pick(&PATHS);
                self.line(&format!("export * from \"{path}\"{}", self.semi()));
            }
            1 => {
                let path = self.rng.pick(&PATHS);
                let name = self.fresh_function_name();
                let alias = self.fresh_function_name();
                self.line(&format!(
                    "export {{ {name} as {alias} }} from \"{path}\"{}",
                    self.semi()
                ));
            }
            _ => {
                let mut specifiers = Vec::new();
                for _ in 0..=self.rng.below(3) {
                    if self.top_level.is_empty() {
                        break;
                    }
                    let local = self.top_level[self.rng.index(self.top_level.len())].clone();
                    let alias = self.fresh("public");
                    specifiers.push(format!("{local} as {alias}"));
                }
                if !specifiers.is_empty() {
                    self.line(&format!(
                        "export {{ {} }}{}",
                        specifiers.join(", "),
                        self.semi()
                    ));
                }
            }
        }
    }

    fn function_declaration(&mut self, frame: Frame, export: Export, depth: u32) {
        let is_async = self.rng.percent(30);
        let generator = !is_async && self.rng.percent(5);
        let name = self.fresh_function_name();
        let export = match export {
            Export::No => "",
            Export::Maybe => self.export_prefix(60),
            Export::Default => "export default ",
        };
        let scope = self.locals.len();
        let mutable = self.mutable.len();
        let parameters = self.parameters(frame);
        if self.rng.percent(40) {
            let names: Vec<String> = self.locals[scope..].to_vec();
            self.jsdoc(&names);
        }
        let keyword = match (is_async, generator) {
            (true, _) => "async function",
            (false, true) => "function*",
            (false, false) => "function",
        };
        self.line(&format!("{export}{keyword} {name}({parameters}) {{"));
        self.body(frame.function(is_async, generator), depth, true);
        self.line("}");
        self.locals.truncate(scope);
        self.mutable.truncate(mutable);
        self.declare(name);
    }

    // Names at indentation zero are module or script bindings; all others,
    // including those of class static blocks, are locals.
    fn declare(&mut self, name: String) {
        if self.indent == 0 {
            self.top_level.push(name);
        } else {
            self.locals.push(name);
        }
    }

    // Formal parameters; their bindings join `locals` for the body.
    fn parameters(&mut self, frame: Frame) -> String {
        let mut parameters = Vec::new();
        for index in 0..self.rng.below(4) {
            let parameter = match self.rng.below(8) {
                0 if index > 0 => {
                    let name = self.fresh("options");
                    let key = self.rng.pick(&PROPERTIES);
                    let default = self.atom(frame);
                    self.locals.push(name.clone());
                    format!("{{ {key}: {name} = {default} }} = {{}}")
                }
                1 => {
                    let name = self.fresh_noun();
                    let default = self.atom(frame);
                    self.locals.push(name.clone());
                    format!("{name} = {default}")
                }
                _ => {
                    let name = self.fresh_noun();
                    self.locals.push(name.clone());
                    name
                }
            };
            parameters.push(parameter);
        }
        if self.rng.percent(5) {
            let name = self.fresh("rest");
            self.locals.push(name.clone());
            parameters.push(format!("...{name}"));
        }
        parameters.join(", ")
    }

    // Statements of a function body, one indentation level deeper.
    fn body(&mut self, frame: Frame, depth: u32, may_return: bool) {
        self.indent += 1;
        let scope = self.locals.len();
        let mutable = self.mutable.len();
        let statements = if depth == 0 { 2 } else { 4 };
        for _ in 0..=self.rng.below(statements) {
            self.statement(frame, depth);
        }
        if frame.function && may_return && self.rng.percent(60) {
            let value = self.expression(frame, 2);
            self.line(&format!("return {value}{}", self.semi()));
        }
        self.locals.truncate(scope);
        self.mutable.truncate(mutable);
        self.indent -= 1;
    }

    fn block(&mut self, frame: Frame, depth: u32) {
        self.body(frame, depth, false);
    }

    fn statement(&mut self, frame: Frame, depth: u32) {
        if self.rng.percent(8) {
            self.comment();
        }
        let simple = depth == 0 || self.indent >= MAX_NESTING;
        match self.rng.below(24) {
            0..=6 => self.variable(frame, depth),
            7..=10 => {
                let statement = self.expression_statement(frame, depth.min(2));
                self.line(&statement);
            }
            11..=12 if !simple => self.if_statement(frame, depth),
            13 if !simple => self.for_of(frame, depth),
            14 if !simple => self.classic_for(frame, depth),
            15 if !simple => self.for_in(frame, depth),
            16 if !simple => self.while_loop(frame, depth),
            17 if !simple => self.switch_statement(frame, depth),
            18 if !simple => self.try_statement(frame, depth),
            19 if frame.function && !simple => {
                self.function_declaration(frame, Export::No, depth - 1)
            }
            20 if frame.generator => {
                let value = self.operand(frame, 1);
                let delegate = if self.rng.percent(20) { "*" } else { "" };
                self.line(&format!("yield{delegate} {value}{}", self.semi()));
            }
            21 if frame.breakable => {
                let condition = self.operand(frame, 1);
                let jump = if frame.in_loop && self.rng.percent(50) {
                    "continue"
                } else {
                    "break"
                };
                self.line(&format!("if ({condition}) {jump}{}", self.semi()));
            }
            22 if !simple && !frame.in_loop => self.labelled_loops(frame, depth),
            _ => self.variable(frame, depth),
        }
    }

    fn variable(&mut self, frame: Frame, depth: u32) {
        match self.rng.below(10) {
            0 | 1 => self.destructuring(frame, depth),
            2 => {
                let name = self.fresh_noun();
                self.line(&format!("let {name}{}", self.semi()));
                self.mutable.push(name.clone());
                self.declare(name);
            }
            _ => {
                let kind = if self.goal == SourceGoal::Script && self.rng.percent(30) {
                    "var"
                } else {
                    self.rng.pick(&["const", "const", "let"])
                };
                let name = self.fresh_noun();
                let value = self.expression(frame, depth.clamp(1, 3));
                self.line(&format!("{kind} {name} = {value}{}", self.semi()));
                if kind != "const" {
                    self.mutable.push(name.clone());
                }
                self.declare(name);
            }
        }
    }

    fn destructuring(&mut self, frame: Frame, depth: u32) {
        let value = self.operand(frame, depth.clamp(1, 2));
        let mut names = Vec::new();
        let pattern = if self.rng.percent(60) {
            let mut properties = Vec::new();
            for _ in 0..=self.rng.below(3) {
                let key = self.rng.pick(&PROPERTIES);
                let name = self.fresh(key);
                if self.rng.percent(30) {
                    let default = self.atom(frame);
                    properties.push(format!("{key}: {name} = {default}"));
                } else {
                    properties.push(format!("{key}: {name}"));
                }
                names.push(name);
            }
            if self.rng.percent(15) {
                let name = self.fresh("rest");
                properties.push(format!("...{name}"));
                names.push(name);
            }
            format!("{{ {} }}", properties.join(", "))
        } else {
            let mut elements = Vec::new();
            for _ in 0..=self.rng.below(3) {
                if self.rng.percent(10) {
                    elements.push(String::new());
                    continue;
                }
                let name = self.fresh_noun();
                elements.push(name.clone());
                names.push(name);
            }
            if self.rng.percent(15) {
                let name = self.fresh("others");
                elements.push(format!("...{name}"));
                names.push(name);
            } else if elements.last().is_some_and(String::is_empty) {
                // A trailing hole needs its comma to remain a hole.
                elements.push(String::new());
            }
            format!("[{}]", elements.join(", "))
        };
        self.line(&format!("const {pattern} = {value}{}", self.semi()));
        for name in names {
            self.declare(name);
        }
    }

    // An expression statement that starts with a name, `this`, or `await`, so
    // omitting semicolons never joins it to the previous line.
    fn expression_statement(&mut self, frame: Frame, depth: u32) -> String {
        let semi = self.semi();
        match self.rng.below(8) {
            0..=2 => format!("{}{semi}", self.call(frame, depth)),
            3 if frame.is_async => format!("await {}{semi}", self.call(frame, depth)),
            4 if !self.mutable.is_empty() => {
                let name = self.mutable[self.rng.index(self.mutable.len())].clone();
                match self.rng.below(3) {
                    0 => format!("{name}++{semi}"),
                    1 => {
                        let operator = self.rng.pick(&["+=", "-=", "||=", "??=", "*="]);
                        let value = self.operand(frame, 1);
                        format!("{name} {operator} {value}{semi}")
                    }
                    _ => {
                        let value = self.expression(frame, depth);
                        format!("{name} = {value}{semi}")
                    }
                }
            }
            _ => {
                let target = self.member_target(frame);
                let value = self.expression(frame, depth);
                format!("{target} = {value}{semi}")
            }
        }
    }

    fn member_target(&mut self, frame: Frame) -> String {
        if frame.in_class && !self.privates.is_empty() && self.rng.percent(50) {
            let private = self.privates[self.rng.index(self.privates.len())].clone();
            return format!("this.{private}");
        }
        let base = if frame.function && self.rng.percent(30) {
            "this".to_owned()
        } else {
            self.reference()
        };
        let property = self.rng.pick(&PROPERTIES);
        format!("{base}.{property}")
    }

    fn if_statement(&mut self, frame: Frame, depth: u32) {
        let condition = self.condition(frame);
        if frame.function && self.rng.percent(25) {
            let value = self.operand(frame, 1);
            self.line(&format!("if ({condition}) return {value}{}", self.semi()));
            return;
        }
        if self.rng.percent(15) {
            let error = self.error_expression(frame);
            self.line(&format!("if ({condition}) {{"));
            self.indent += 1;
            self.line(&format!("throw {error}{}", self.semi()));
            self.indent -= 1;
            self.line("}");
            return;
        }
        self.line(&format!("if ({condition}) {{"));
        self.block(frame, depth - 1);
        if self.rng.percent(25) {
            let condition = self.condition(frame);
            self.line(&format!("}} else if ({condition}) {{"));
            self.block(frame, depth - 1);
        }
        if self.rng.percent(40) {
            self.line("} else {");
            self.block(frame, depth - 1);
        }
        self.line("}");
    }

    fn condition(&mut self, frame: Frame) -> String {
        match self.rng.below(4) {
            0 => self.operand(frame, 1),
            _ => self.binary(frame, 1),
        }
    }

    fn error_expression(&mut self, frame: Frame) -> String {
        let constructor = self.rng.pick(&["Error", "TypeError", "RangeError"]);
        let message = self.template(frame, 1);
        format!("new {constructor}({message})")
    }

    fn for_of(&mut self, frame: Frame, depth: u32) {
        let iterable = self.operand(frame, 1);
        let name = self.fresh_noun();
        let awaited = if frame.is_async && self.rng.percent(20) {
            "await "
        } else {
            ""
        };
        self.line(&format!("for {awaited}(const {name} of {iterable}) {{"));
        self.locals.push(name);
        self.block(frame.looping(), depth - 1);
        self.locals.pop();
        self.line("}");
    }

    fn classic_for(&mut self, frame: Frame, depth: u32) {
        let index = self.fresh("index");
        let bound = self.member_chain(frame, 1);
        self.line(&format!(
            "for (let {index} = 0; {index} < {bound}; {index} += 1) {{"
        ));
        self.locals.push(index);
        self.block(frame.looping(), depth - 1);
        self.locals.pop();
        self.line("}");
    }

    fn for_in(&mut self, frame: Frame, depth: u32) {
        let object = self.reference();
        let key = self.fresh("key");
        self.line(&format!("for (const {key} in {object}) {{"));
        self.locals.push(key);
        self.block(frame.looping(), depth - 1);
        self.locals.pop();
        self.line("}");
    }

    fn while_loop(&mut self, frame: Frame, depth: u32) {
        let condition = self.condition(frame);
        if self.rng.percent(30) {
            self.line("do {");
            self.block(frame.looping(), depth - 1);
            self.line(&format!("}} while ({condition}){}", self.semi()));
        } else {
            self.line(&format!("while ({condition}) {{"));
            self.block(frame.looping(), depth - 1);
            self.indent += 1;
            self.line(&format!("break{}", self.semi()));
            self.indent -= 1;
            self.line("}");
        }
    }

    fn labelled_loops(&mut self, frame: Frame, depth: u32) {
        let label = self.fresh("outer");
        let row = self.fresh("row");
        let cell = self.fresh("cell");
        let rows = self.reference();
        self.line(&format!("{label}: for (const {row} of {rows}) {{"));
        self.indent += 1;
        self.line(&format!("for (const {cell} of {row}) {{"));
        self.indent += 1;
        let condition = self.operand(frame, 1);
        let jump = self.rng.pick(&["continue", "break"]);
        self.line(&format!("if ({condition}) {jump} {label}{}", self.semi()));
        self.indent -= 1;
        self.line("}");
        self.indent -= 1;
        self.locals.push(row);
        self.block(frame.looping(), depth.saturating_sub(2));
        self.locals.pop();
        self.line("}");
    }

    fn switch_statement(&mut self, frame: Frame, depth: u32) {
        let discriminant = self.operand(frame, 1);
        self.line(&format!("switch ({discriminant}) {{"));
        let inner = Frame {
            breakable: true,
            ..frame
        };
        for case in 0..=self.rng.below(4) {
            let test = if self.rng.percent(50) {
                self.rng.pick(&STRINGS).to_owned()
            } else {
                case.to_string()
            };
            self.indent += 1;
            self.line(&format!("case {test}:"));
            self.indent -= 1;
            self.indent += 1;
            self.block(inner, depth.saturating_sub(2));
            self.indent += 1;
            self.line(&format!("break{}", self.semi()));
            self.indent -= 1;
            self.indent -= 1;
        }
        if self.rng.percent(60) {
            self.indent += 1;
            self.line("default:");
            self.block(inner, 0);
            self.indent -= 1;
        }
        self.line("}");
    }

    fn try_statement(&mut self, frame: Frame, depth: u32) {
        self.line("try {");
        self.block(frame, depth - 1);
        if self.rng.percent(15) {
            self.line("} catch {");
            self.block(frame, depth - 1);
        } else {
            let error = self.fresh("error");
            self.line(&format!("}} catch ({error}) {{"));
            self.locals.push(error);
            self.block(frame, depth - 1);
            self.locals.pop();
        }
        if self.rng.percent(25) {
            self.line("} finally {");
            self.block(frame, 0);
        }
        self.line("}");
    }

    fn class_declaration(&mut self) {
        let export = self.export_prefix(60);
        let noun = capitalize(self.rng.pick(&NOUNS));
        let suffix = self.rng.pick(&CLASS_SUFFIXES);
        let name = self.fresh(&format!("{noun}{suffix}"));
        let base = if !self.classes.is_empty() && self.rng.percent(30) {
            Some(self.classes[self.rng.index(self.classes.len())].clone())
        } else {
            None
        };
        if self.rng.percent(30) {
            self.jsdoc(&[]);
        }
        let heritage = match &base {
            Some(base) => format!(" extends {base}"),
            None => String::new(),
        };
        self.line(&format!("{export}class {name}{heritage} {{"));
        self.indent += 1;
        let frame = Frame {
            in_class: true,
            ..Frame::TOP
        };
        self.privates.clear();
        // Private names may be used before their declaration, so all are
        // chosen first.
        let fields = self.rng.below(4);
        let mut private_fields = Vec::new();
        for _ in 0..fields {
            if self.rng.percent(60) {
                let noun = self.rng.pick(&NOUNS);
                private_fields.push(self.fresh(&format!("#{noun}")));
            }
        }
        let mut private_method_names = Vec::new();
        for _ in 0..self.rng.below(2) {
            let verb = self.rng.pick(&VERBS);
            private_method_names.push(self.fresh(&format!("#{verb}")));
        }
        self.privates.extend(private_fields.iter().cloned());
        self.privates.extend(private_method_names.iter().cloned());
        for private in &private_fields {
            let is_static = if self.rng.percent(20) { "static " } else { "" };
            let value = self.operand(frame, 1);
            let line = format!("{is_static}{private} = {value};");
            self.line(&line);
        }
        for _ in private_fields.len() as u64..fields {
            let key = self.rng.pick(&PROPERTIES);
            let is_static = if self.rng.percent(20) { "static " } else { "" };
            if self.rng.percent(20) {
                self.line(&format!("{is_static}{key};"));
            } else {
                let value = self.expression(frame, 2);
                self.line(&format!("{is_static}{key} = {value};"));
            }
        }
        if self.rng.percent(70) {
            self.constructor(frame, base.is_some());
        }
        for private in private_method_names {
            self.method(frame, &private, base.is_some());
        }
        for _ in 0..=self.rng.below(3) {
            let method = self.rng.pick(&VERBS);
            self.method(frame, method, base.is_some());
        }
        if self.rng.percent(10) {
            self.line("static {");
            self.body(frame, 1, false);
            self.line("}");
        }
        self.indent -= 1;
        self.line("}");
        self.privates.clear();
        self.classes.push(name.clone());
        self.top_level.push(name);
    }

    fn constructor(&mut self, frame: Frame, derived: bool) {
        let scope = self.locals.len();
        let parameters = self.parameters(frame);
        self.line(&format!("constructor({parameters}) {{"));
        self.indent += 1;
        if derived {
            let arguments = self.arguments(frame, 1);
            self.line(&format!("super({arguments}){}", self.semi()));
        }
        for index in scope..self.locals.len() {
            let property = self.rng.pick(&PROPERTIES);
            let line = format!("this.{property} = {}{}", self.locals[index], self.semi());
            self.line(&line);
        }
        self.indent -= 1;
        self.body(frame.function(false, false), 1, false);
        self.line("}");
        self.locals.truncate(scope);
    }

    fn method(&mut self, frame: Frame, name: &str, derived: bool) {
        let is_static = if self.rng.percent(15) { "static " } else { "" };
        let scope = self.locals.len();
        let (prefix, parameters, is_async, generator, may_return) = match self.rng.below(10) {
            0 if !name.starts_with('#') => ("get ", String::new(), false, false, true),
            1 if !name.starts_with('#') => {
                let value = self.fresh("value");
                self.locals.push(value.clone());
                ("set ", value, false, false, false)
            }
            2 | 3 => ("async ", self.parameters(frame), true, false, true),
            4 => ("*", self.parameters(frame), false, true, false),
            _ => ("", self.parameters(frame), false, false, true),
        };
        self.line(&format!("{is_static}{prefix}{name}({parameters}) {{"));
        let inner = frame
            .function(is_async, generator)
            .with_super(derived && is_static.is_empty());
        if derived && is_static.is_empty() && self.rng.percent(20) {
            self.indent += 1;
            self.line(&format!("super.{name}?.(){}", self.semi()).replace("super.#", "this.#"));
            self.indent -= 1;
        }
        self.body(inner, 2, may_return);
        self.line("}");
        self.locals.truncate(scope);
    }

    fn reference(&mut self) -> String {
        match self.rng.below(4) {
            0 | 1 if !self.locals.is_empty() => {
                self.locals[self.rng.index(self.locals.len())].clone()
            }
            2 if !self.top_level.is_empty() => {
                self.top_level[self.rng.index(self.top_level.len())].clone()
            }
            _ => self.rng.pick(&GLOBALS).to_owned(),
        }
    }

    fn atom(&mut self, frame: Frame) -> String {
        match self.rng.below(12) {
            0..=3 => self.reference(),
            4 | 5 => self.rng.pick(&NUMBERS).to_owned(),
            6..=8 => self.rng.pick(&STRINGS).to_owned(),
            9 => self
                .rng
                .pick(&["true", "false", "null", "undefined"])
                .to_owned(),
            10 if frame.function => "this".to_owned(),
            _ => self.rng.pick(&REGEXPS).to_owned(),
        }
    }

    // An expression that is valid as an operand of any binary operator
    // without parentheses. Arrow functions and assignments never are.
    fn operand(&mut self, frame: Frame, depth: u32) -> String {
        if depth == 0 {
            return self.atom(frame);
        }
        match self.rng.below(16) {
            0..=3 => self.atom(frame),
            4..=6 => self.member_chain(frame, depth),
            7 => self.call(frame, depth),
            8 => self.object_literal(frame, depth),
            9 => self.array_literal(frame, depth),
            10 => self.template(frame, depth),
            11 => format!("({})", self.binary(frame, depth - 1)),
            12 => self.unary(frame, depth),
            13 => self.new_expression(frame, depth),
            14 if frame.is_async => format!("await {}", self.call(frame, depth - 1)),
            _ => self.member_chain(frame, depth),
        }
    }

    fn expression(&mut self, frame: Frame, depth: u32) -> String {
        if depth == 0 {
            return self.atom(frame);
        }
        match self.rng.below(12) {
            0..=4 => self.operand(frame, depth),
            5 | 6 => self.binary(frame, depth),
            7 => {
                let test = self.operand(frame, depth - 1);
                let consequent = self.operand(frame, depth - 1);
                let alternate = self.operand(frame, depth - 1);
                format!("{test} ? {consequent} : {alternate}")
            }
            8 | 9 => self.arrow(frame, depth),
            10 if self.indent < MAX_NESTING && self.rng.percent(30) => {
                self.function_expression(frame)
            }
            _ => self.operand(frame, depth),
        }
    }

    fn binary(&mut self, frame: Frame, depth: u32) -> String {
        let left = self.operand(frame, depth.saturating_sub(1));
        if frame.in_class && !self.privates.is_empty() && self.rng.percent(5) {
            let private = self.privates[self.rng.index(self.privates.len())].clone();
            return format!("{private} in {left}");
        }
        if !self.classes.is_empty() && self.rng.percent(5) {
            let class = self.classes[self.rng.index(self.classes.len())].clone();
            return format!("{left} instanceof {class}");
        }
        let operator = self.rng.pick(&BINARY_OPERATORS);
        let right = self.operand(frame, depth.saturating_sub(1));
        format!("{left} {operator} {right}")
    }

    fn unary(&mut self, frame: Frame, depth: u32) -> String {
        match self.rng.below(4) {
            0 => format!("!{}", self.operand(frame, depth - 1)),
            1 => format!("typeof {}", self.operand(frame, depth - 1)),
            2 => format!("-{}", self.reference()),
            _ => "void 0".to_owned(),
        }
    }

    fn member_chain(&mut self, frame: Frame, depth: u32) -> String {
        let mut chain = if frame.in_class && !self.privates.is_empty() && self.rng.percent(30) {
            let private = self.privates[self.rng.index(self.privates.len())].clone();
            format!("this.{private}")
        } else if frame.super_property && self.rng.percent(10) {
            let property = self.rng.pick(&PROPERTIES);
            format!("super.{property}")
        } else {
            self.reference()
        };
        for _ in 0..=self.rng.below(3) {
            match self.rng.below(8) {
                0..=2 => {
                    chain.push('.');
                    chain.push_str(self.rng.pick(&PROPERTIES));
                }
                3 => {
                    chain.push_str("?.");
                    chain.push_str(self.rng.pick(&PROPERTIES));
                }
                4 => {
                    let index = self.atom(frame);
                    chain.push_str(&format!("[{index}]"));
                }
                5 | 6 => {
                    let method = self.rng.pick(&METHODS);
                    let arguments = self.arguments(frame, depth.saturating_sub(1));
                    chain.push_str(&format!(".{method}({arguments})"));
                }
                _ => {
                    let method = self.rng.pick(&METHODS);
                    chain.push_str(&format!("?.{method}?.()"));
                }
            }
        }
        chain
    }

    fn call(&mut self, frame: Frame, depth: u32) -> String {
        let callee = match self.rng.below(3) {
            0 => self.reference(),
            _ => {
                let base = self.reference();
                let method = self.rng.pick(&METHODS);
                format!("{base}.{method}")
            }
        };
        let arguments = self.arguments(frame, depth.saturating_sub(1));
        format!("{callee}({arguments})")
    }

    fn arguments(&mut self, frame: Frame, depth: u32) -> String {
        let mut arguments = Vec::new();
        for _ in 0..self.rng.below(4) {
            if self.rng.percent(5) {
                arguments.push(format!("...{}", self.reference()));
            } else {
                arguments.push(self.expression(frame, depth));
            }
        }
        arguments.join(", ")
    }

    fn new_expression(&mut self, frame: Frame, depth: u32) -> String {
        let constructor = if !self.classes.is_empty() && self.rng.percent(30) {
            self.classes[self.rng.index(self.classes.len())].clone()
        } else {
            self.rng.pick(&CONSTRUCTORS).to_owned()
        };
        let arguments = self.arguments(frame, depth.saturating_sub(1));
        format!("new {constructor}({arguments})")
    }

    fn object_literal(&mut self, frame: Frame, depth: u32) -> String {
        let count = self.rng.below(5);
        if count == 0 {
            return "{}".to_owned();
        }
        let mut properties = Vec::new();
        for _ in 0..count {
            let key = self.rng.pick(&PROPERTIES);
            let property = match self.rng.below(12) {
                0 if !self.locals.is_empty() => {
                    self.locals[self.rng.index(self.locals.len())].clone()
                }
                1 => format!("...{}", self.reference()),
                2 => {
                    let computed = self.atom(frame);
                    let value = self.operand(frame, depth.saturating_sub(1));
                    format!("[{computed}]: {value}")
                }
                3 => {
                    let quoted = self.rng.pick(&STRINGS);
                    let value = self.operand(frame, depth.saturating_sub(1));
                    format!("{quoted}: {value}")
                }
                4 if depth > 1 && self.indent < MAX_NESTING => self.object_method(frame, key),
                _ => {
                    let value = self.expression(frame, depth.saturating_sub(1));
                    format!("{key}: {value}")
                }
            };
            properties.push(property);
        }
        if properties.len() <= 2 && !properties.iter().any(|item| item.contains('\n')) {
            return format!("{{ {} }}", properties.join(", "));
        }
        let mut text = String::from("{");
        text.push_str(self.newline);
        let inner = self.indent_unit.repeat(self.indent + 1);
        for property in properties {
            text.push_str(&inner);
            text.push_str(&property);
            text.push(',');
            text.push_str(self.newline);
        }
        text.push_str(&self.indent_unit.repeat(self.indent));
        text.push('}');
        text
    }

    fn object_method(&mut self, frame: Frame, key: &str) -> String {
        let prefix = self.rng.pick(&["", "", "async ", "get "]);
        let is_async = prefix == "async ";
        let scope = self.locals.len();
        let parameters = if prefix == "get " {
            String::new()
        } else {
            self.parameters(frame)
        };
        let body = self.nested_block(frame.function(is_async, false), 1, true);
        self.locals.truncate(scope);
        format!("{prefix}{key}({parameters}) {body}")
    }

    fn array_literal(&mut self, frame: Frame, depth: u32) -> String {
        let mut elements = Vec::new();
        for _ in 0..self.rng.below(5) {
            if self.rng.percent(10) {
                elements.push(format!("...{}", self.reference()));
            } else {
                elements.push(self.operand(frame, depth.saturating_sub(1)));
            }
        }
        format!("[{}]", elements.join(", "))
    }

    fn template(&mut self, frame: Frame, depth: u32) -> String {
        let tag = if self.rng.percent(10) {
            self.rng.pick(&["css", "html", "String.raw"])
        } else {
            ""
        };
        let mut text = format!("{tag}`");
        text.push_str(self.rng.pick(&TEMPLATE_TEXTS));
        for _ in 0..=self.rng.below(2) {
            let substitution = self.operand(frame, depth.saturating_sub(1));
            text.push_str(&format!("${{{substitution}}}"));
            text.push_str(self.rng.pick(&TEMPLATE_TEXTS));
        }
        text.push('`');
        text
    }

    fn arrow(&mut self, frame: Frame, depth: u32) -> String {
        let is_async = self.rng.percent(20);
        let scope = self.locals.len();
        let parameters = self.parameters(frame);
        let inner = frame.arrow(is_async);
        let head = if is_async {
            format!("async ({parameters}) =>")
        } else if self.rng.percent(30)
            && !parameters.is_empty()
            && !parameters.contains([',', '=', '{', '.'])
        {
            format!("{parameters} =>")
        } else {
            format!("({parameters}) =>")
        };
        let arrow = if self.indent < MAX_NESTING && (is_async || self.rng.percent(30)) {
            let body = self.nested_block(inner, depth.min(2), true);
            format!("{head} {body}")
        } else {
            let body = match self.rng.below(3) {
                0 => self.binary(inner, depth - 1),
                _ => self.operand(inner, depth - 1),
            };
            if body.starts_with('{') {
                format!("{head} ({body})")
            } else {
                format!("{head} {body}")
            }
        };
        self.locals.truncate(scope);
        arrow
    }

    fn function_expression(&mut self, frame: Frame) -> String {
        let scope = self.locals.len();
        let parameters = self.parameters(frame);
        let name = if self.rng.percent(50) {
            format!(" {}", self.fresh_function_name())
        } else {
            String::new()
        };
        let body = self.nested_block(frame.function(false, false), 1, true);
        self.locals.truncate(scope);
        format!("function{name}({parameters}) {body}")
    }

    // A braced block generated into a string at the current indentation, for
    // functions nested in expressions.
    fn nested_block(&mut self, frame: Frame, depth: u32, may_return: bool) -> String {
        let outer = std::mem::take(&mut self.out);
        self.out.push('{');
        self.out.push_str(self.newline);
        self.body(frame, depth, may_return);
        for _ in 0..self.indent {
            self.out.push_str(self.indent_unit);
        }
        self.out.push('}');
        std::mem::replace(&mut self.out, outer)
    }
}

fn capitalize(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_engine::{ParseLimits, SourceText, TextLimits, parse_js_program};

    #[test]
    fn rng_matches_the_splitmix64_reference_sequence() {
        let mut rng = Rng::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
        let mut rng = Rng::new(7);
        assert!((0..1_000).all(|_| rng.below(13) < 13));
    }

    #[test]
    fn generation_is_deterministic_and_seed_dependent() {
        let first = Corpus::mixed(3, 8);
        let second = Corpus::mixed(3, 8);
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert!(
            first
                .files
                .iter()
                .zip(&second.files)
                .all(|(a, b)| a.text == b.text && a.goal == b.goal)
        );
        assert_ne!(first.fingerprint(), Corpus::mixed(4, 8).fingerprint());
    }

    #[test]
    fn files_reach_their_target_size() {
        for (seed, target) in [(1, 0), (2, 1), (3, 1_024), (4, 20_000)] {
            for goal in [SourceGoal::Script, SourceGoal::Module] {
                let file = generate_file(seed, goal, target);
                assert!(file.text.len() >= target);
                assert!(file.text.len() < target + 8_192, "{}", file.text.len());
            }
        }
    }

    // Pins the default corpus, so a platform or generator change that alters
    // its bytes is detected. A deliberate change bumps GENERATOR_VERSION.
    #[test]
    fn default_corpus_fingerprint_is_pinned() {
        let corpus = Corpus::mixed(DEFAULT_SEED, 1_000);
        assert_eq!(GENERATOR_VERSION, 1);
        assert_eq!(corpus.bytes(), 10_342_331);
        assert_eq!(corpus.fingerprint(), 0x7C71_C5E6_160D_3383);
    }

    // The harness rejects a corpus with a syntax error; this keeps generator
    // changes from reaching it.
    #[test]
    fn generated_files_parse_in_their_goal() {
        for seed in 0..40 {
            let corpus = Corpus::mixed(seed, 6);
            for (index, file) in corpus.files.iter().enumerate() {
                let source = SourceText::new(file.text.clone(), TextLimits::default())
                    .expect("generated text is within the default limits");
                if let Err(error) = parse_js_program(&source, file.goal, ParseLimits::default()) {
                    let start = error.offset.saturating_sub(80);
                    let end = (error.offset + 80).min(file.text.len());
                    panic!(
                        "seed {seed}, file {index}: {error:?}\n{}",
                        file.text.get(start..end).unwrap_or_default()
                    );
                }
            }
        }
    }
}
