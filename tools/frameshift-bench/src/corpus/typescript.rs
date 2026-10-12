//! TypeScript, TSX, and declaration-file generation for the E1 corpus (`D-043`).
//!
//! These methods extend the JavaScript emitter only when it generates
//! TypeScript, so the JavaScript and JSX corpora keep their bytes. They emit
//! type annotations, generics, interfaces, type aliases, enums, namespaces,
//! overloads, typed classes, ambient declarations, and the TypeScript
//! expressions `as`, `satisfies`, `!`, angle-bracket assertions, and type
//! arguments. Each construct is one that the pinned `typescript` 6.0.3 parser
//! accepts in the file's dialect.

use super::{
    CLASS_SUFFIXES, COMMENTS, Emitter, Export, Frame, NOUNS, PATHS, PROPERTIES, STRINGS, VERBS,
    capitalize,
};

const TYPE_KEYWORDS: [&str; 12] = [
    "string",
    "number",
    "boolean",
    "unknown",
    "bigint",
    "symbol",
    "undefined",
    "null",
    "void",
    "never",
    "object",
    "any",
];

// Global generic types with their number of type arguments.
const GENERIC_TYPES: [(&str, usize); 12] = [
    ("Array", 1),
    ("Promise", 1),
    ("Record", 2),
    ("Partial", 1),
    ("Readonly", 1),
    ("Map", 2),
    ("Set", 1),
    ("ReadonlyArray", 1),
    ("Required", 1),
    ("NonNullable", 1),
    ("Awaited", 1),
    ("Iterable", 1),
];

const LITERAL_TYPES: [&str; 10] = [
    "\"primary\"",
    "'secondary'",
    "\"en-US\"",
    "42",
    "-1",
    "0n",
    "true",
    "false",
    "null",
    "`static`",
];

const TYPE_SUFFIXES: [&str; 8] = [
    "Props", "Options", "State", "Config", "Result", "Item", "Entry", "Context",
];

const DECORATORS: [&str; 6] = [
    "sealed",
    "observable",
    "memoize()",
    "Injectable()",
    "route(\"/items\")",
    "logged",
];

const ACCESSIBILITY: [&str; 3] = ["private", "protected", "public"];

// What a generated type must be wrapped in before it can appear as an
// operand of `|`, `&`, `[]`, indexed access, or a conditional's check type.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeForm {
    // Primary types: names, references, literals, tuples, object types.
    Atomic,
    // `keyof`, `readonly`, unions, and intersections.
    Operator,
    // Function and constructor types, which extend as far as possible.
    Function,
    // Conditional types.
    Conditional,
}

impl Emitter {
    /// Generates a type of bounded depth.
    pub(super) fn ty(&mut self, depth: u32) -> String {
        self.ty_form(depth).0
    }

    // A type that is safe as an operand of a type operator.
    pub(super) fn ty_atom(&mut self, depth: u32) -> String {
        match self.ty_form(depth) {
            (text, TypeForm::Atomic) => text,
            (text, TypeForm::Operator | TypeForm::Function | TypeForm::Conditional) => {
                format!("({text})")
            }
        }
    }

    // A type for the return type of an arrow function, where a function or
    // conditional type would capture the arrow, but a union such as
    // `T | null` stays unparenthesized as in real code.
    fn arrow_return_type(&mut self, depth: u32) -> String {
        match self.ty_form(depth) {
            (text, TypeForm::Atomic | TypeForm::Operator) => text,
            (text, TypeForm::Function | TypeForm::Conditional) => format!("({text})"),
        }
    }

    fn simple_type(&mut self) -> String {
        match self.rng.below(10) {
            0..=2 => self.rng.pick(&TYPE_KEYWORDS).to_owned(),
            3..=5 if !self.types.is_empty() => self.types[self.rng.index(self.types.len())].clone(),
            6 | 7 if !self.type_params.is_empty() => {
                self.type_params[self.rng.index(self.type_params.len())].clone()
            }
            8 => self.rng.pick(&LITERAL_TYPES).to_owned(),
            _ => self.rng.pick(&TYPE_KEYWORDS).to_owned(),
        }
    }

    fn ty_form(&mut self, depth: u32) -> (String, TypeForm) {
        if depth == 0 {
            return (self.simple_type(), TypeForm::Atomic);
        }
        let inner = depth - 1;
        match self.rng.below(24) {
            0..=7 => (self.simple_type(), TypeForm::Atomic),
            8 => (format!("{}[]", self.ty_atom(inner)), TypeForm::Atomic),
            9 | 10 => {
                let mut members = Vec::new();
                for _ in 0..2 + self.rng.below(2) {
                    members.push(self.ty_atom(inner));
                }
                let leading = if self.rng.percent(5) { "| " } else { "" };
                (
                    format!("{leading}{}", members.join(" | ")),
                    TypeForm::Operator,
                )
            }
            11 => {
                let left = self.ty_atom(inner);
                let right = self.ty_atom(inner);
                (format!("{left} & {right}"), TypeForm::Operator)
            }
            12 | 13 => {
                let (name, arity) = self.rng.pick(&GENERIC_TYPES);
                let arguments: Vec<String> = (0..arity).map(|_| self.ty(inner)).collect();
                (
                    format!("{name}<{}>", arguments.join(", ")),
                    TypeForm::Atomic,
                )
            }
            14 => (self.tuple_type(inner), TypeForm::Atomic),
            15 => (self.type_literal(inner), TypeForm::Atomic),
            16 => (self.function_type(inner), TypeForm::Function),
            17 => {
                let first = self.rng.pick(&LITERAL_TYPES);
                let second = self.rng.pick(&STRINGS);
                (format!("{first} | {second}"), TypeForm::Operator)
            }
            18 => {
                let operand = if self.locals.is_empty() || self.rng.percent(50) {
                    self.ty_atom(inner)
                } else {
                    let local = self.locals[self.rng.index(self.locals.len())].clone();
                    format!("typeof {local}")
                };
                (format!("keyof {operand}"), TypeForm::Operator)
            }
            19 => {
                let object = self.ty_atom(inner);
                let index = match self.rng.below(3) {
                    0 => "number".to_owned(),
                    1 => self.ty_atom(0),
                    _ => format!("\"{}\"", self.rng.pick(&PROPERTIES)),
                };
                (format!("{object}[{index}]"), TypeForm::Atomic)
            }
            20 => {
                let head = self.rng.pick(&NOUNS);
                let middle = self.ty(inner);
                (
                    format!("`{head}-${{{middle}}}-${{number}}`"),
                    TypeForm::Atomic,
                )
            }
            21 => (self.conditional_type(inner), TypeForm::Conditional),
            22 => (self.mapped_type(inner), TypeForm::Atomic),
            _ => (
                format!("readonly {}[]", self.ty_atom(inner)),
                TypeForm::Operator,
            ),
        }
    }

    fn tuple_type(&mut self, depth: u32) -> String {
        let labeled = self.rng.percent(30);
        let mut elements = Vec::new();
        let mut optional = false;
        for _ in 0..self.rng.below(4) {
            optional = optional || self.rng.percent(20);
            let mark = if optional { "?" } else { "" };
            if labeled {
                let label = self.fresh_noun();
                let ty = self.ty(depth);
                elements.push(format!("{label}{mark}: {ty}"));
            } else {
                let ty = if optional {
                    self.ty_atom(depth)
                } else {
                    self.ty(depth)
                };
                elements.push(format!("{ty}{mark}"));
            }
        }
        if self.rng.percent(15) {
            let element = self.ty_atom(depth);
            if labeled {
                let label = self.fresh("rest");
                elements.push(format!("...{label}: {element}[]"));
            } else {
                elements.push(format!("...{element}[]"));
            }
        }
        format!("[{}]", elements.join(", "))
    }

    fn type_literal(&mut self, depth: u32) -> String {
        let mut members = Vec::new();
        for _ in 0..self.rng.below(4) {
            let member = match self.rng.below(8) {
                0 => format!("[key: string]: {}", self.ty(depth)),
                1 => {
                    let parameters = self.signature_parameters(depth);
                    let result = self.ty(depth);
                    format!("({parameters}): {result}")
                }
                2 => {
                    let name = self.rng.pick(&VERBS);
                    let parameters = self.signature_parameters(depth);
                    let result = self.ty(depth);
                    format!("{name}({parameters}): {result}")
                }
                3 => format!(
                    "readonly {}: {}",
                    self.rng.pick(&PROPERTIES),
                    self.ty(depth)
                ),
                4 => format!("{}?: {}", self.rng.pick(&PROPERTIES), self.ty(depth)),
                _ => format!("{}: {}", self.rng.pick(&PROPERTIES), self.ty(depth)),
            };
            members.push(member);
        }
        if members.is_empty() {
            return "{}".to_owned();
        }
        let separator = if self.rng.percent(70) { "; " } else { ", " };
        format!("{{ {} }}", members.join(separator))
    }

    fn function_type(&mut self, depth: u32) -> String {
        let scope = self.type_params.len();
        let generics = if self.rng.percent(15) {
            self.type_parameters(false)
        } else {
            String::new()
        };
        let parameters = self.signature_parameters(depth);
        let result = self.ty(depth);
        self.type_params.truncate(scope);
        let constructor = match self.rng.below(12) {
            0 => "new ",
            1 => "abstract new ",
            _ => "",
        };
        format!("{constructor}{generics}({parameters}) => {result}")
    }

    fn conditional_type(&mut self, depth: u32) -> String {
        let check = self.ty_atom(depth);
        if self.rng.percent(40) {
            let inferred = self.fresh("U");
            let (wrapper, _) = self.rng.pick(&GENERIC_TYPES[..2]);
            let constraint = if self.rng.percent(30) {
                format!(" extends {}", self.rng.pick(&TYPE_KEYWORDS[..2]))
            } else {
                String::new()
            };
            let alternate = self.ty(depth);
            return format!(
                "{check} extends {wrapper}<infer {inferred}{constraint}> ? {inferred} : {alternate}"
            );
        }
        let extends = self.ty_atom(depth);
        let consequent = self.ty(depth);
        let alternate = self.ty(depth);
        format!("{check} extends {extends} ? {consequent} : {alternate}")
    }

    fn mapped_type(&mut self, depth: u32) -> String {
        let key = self.fresh("K");
        let source = self.ty_atom(depth);
        let readonly = self
            .rng
            .pick(&["", "", "readonly ", "-readonly ", "+readonly "]);
        let optional = self.rng.pick(&["", "", "?", "-?", "+?"]);
        let remapping = if self.rng.percent(20) {
            format!(" as `get${{Capitalize<{key} & string>}}`")
        } else {
            String::new()
        };
        format!("{{ {readonly}[{key} in keyof {source}{remapping}]{optional}: {source}[{key}] }}")
    }

    // Parameters of a signature without a body, which take no initializers.
    fn signature_parameters(&mut self, depth: u32) -> String {
        let mut parameters = Vec::new();
        let count = self.rng.below(3);
        for index in 0..count {
            let name = self.fresh_noun();
            let mark = if index + 1 == count && self.rng.percent(25) {
                "?"
            } else {
                ""
            };
            let ty = self.ty(depth);
            parameters.push(format!("{name}{mark}: {ty}"));
        }
        if self.rng.percent(10) {
            let name = self.fresh("rest");
            let element = self.ty_atom(depth);
            parameters.push(format!("...{name}: {element}[]"));
        }
        parameters.join(", ")
    }

    /// Generates a type parameter list and brings its names into scope. A
    /// TSX arrow function needs a trailing comma or a constraint to tell its
    /// parameter list from an element.
    pub(super) fn type_parameters(&mut self, tsx_arrow: bool) -> String {
        let count = 1 + self.rng.below(2);
        let mut parameters = Vec::new();
        for index in 0..count {
            let name = self.fresh("T");
            let modifier = if self.rng.percent(10) { "const " } else { "" };
            let constraint = if self.rng.percent(40) {
                format!(" extends {}", self.ty_atom(1))
            } else {
                String::new()
            };
            // Only the last parameter has a default, so no required parameter
            // follows an optional one.
            let default = if index + 1 == count && self.rng.percent(20) {
                format!(" = {}", self.ty(1))
            } else {
                String::new()
            };
            parameters.push(format!("{modifier}{name}{constraint}{default}"));
            self.type_params.push(name);
        }
        let trailing = if tsx_arrow && count == 1 && !parameters[0].contains(" extends ") {
            ","
        } else {
            ""
        };
        format!("<{}{trailing}>", parameters.join(", "))
    }

    /// A `: Type` annotation with the given chance, or nothing.
    pub(super) fn annotation(&mut self, chance: u64) -> String {
        if self.rng.percent(chance) {
            format!(": {}", self.ty(2))
        } else {
            String::new()
        }
    }

    /// The annotation of one formal parameter, chosen when the caller knows
    /// whether the parameter is the last one and has a default.
    pub(super) fn parameter_annotation(&mut self, last: bool, has_default: bool) -> String {
        if !self.rng.percent(75) {
            return String::new();
        }
        let mark = if last && !has_default && self.rng.percent(20) {
            "?"
        } else {
            ""
        };
        format!("{mark}: {}", self.ty(2))
    }

    /// The return type of a function declaration or method.
    pub(super) fn return_annotation(&mut self, is_async: bool, generator: bool) -> String {
        if !self.rng.percent(55) {
            return String::new();
        }
        let ty = self.ty(2);
        match (is_async, generator) {
            (true, _) => format!(": Promise<{ty}>"),
            (false, true) => format!(": Generator<{ty}>"),
            (false, false) => format!(": {ty}"),
        }
    }

    /// Generics and a return type for an arrow function, empty outside
    /// TypeScript.
    pub(super) fn arrow_types(&mut self) -> (String, String) {
        let generics = if self.rng.percent(10) {
            self.type_parameters(self.jsx)
        } else {
            String::new()
        };
        let result = if self.rng.percent(30) {
            format!(": {}", self.arrow_return_type(2))
        } else {
            String::new()
        };
        (generics, result)
    }

    /// An operand that uses TypeScript expression syntax.
    pub(super) fn ts_operand(&mut self, frame: Frame, depth: u32) -> String {
        let inner = depth.saturating_sub(1);
        match self.rng.below(6) {
            0 => {
                let value = self.operand(frame, inner);
                let ty = self.ty(1);
                format!("({value} as {ty})")
            }
            1 => {
                let value = self.operand(frame, inner);
                let ty = self.ty(1);
                format!("({value} satisfies {ty})")
            }
            2 => format!("{}!", self.member_chain(frame, inner.max(1))),
            // Assertions name their type, as real code does. With a tuple
            // or object type, the compiler can misread `a < <[T]>/re/` as
            // type arguments of `a` and keep that reading's errors.
            3 if !self.jsx => {
                let ty = if !self.types.is_empty() && self.rng.percent(50) {
                    self.types[self.rng.index(self.types.len())].clone()
                } else {
                    self.rng.pick(&TYPE_KEYWORDS).to_owned()
                };
                let value = self.atom(frame);
                format!("<{ty}>{value}")
            }
            4 => {
                let callee = self.reference();
                let ty = self.ty(1);
                let arguments = self.arguments(frame, inner);
                format!("{callee}<{ty}>({arguments})")
            }
            _ => {
                let key = self.rng.pick(&["string", "number"]);
                let value = self.ty(1);
                format!("new Map<{key}, {value}>()")
            }
        }
    }

    /// A complete expression that ends with `as`, `as const`, or `satisfies`.
    pub(super) fn ts_expression(&mut self, frame: Frame, depth: u32) -> String {
        match self.rng.below(3) {
            0 => {
                let value = self.object_literal(frame, depth);
                let ty = self.ty(2);
                format!("{value} satisfies {ty}")
            }
            1 => format!("{} as const", self.array_literal(frame, depth)),
            _ => {
                let value = self.operand(frame, depth);
                let ty = self.ty(2);
                format!("{value} as {ty}")
            }
        }
    }

    /// Type-only imports and import-equals declarations.
    pub(super) fn ts_imports(&mut self) {
        for _ in 0..self.rng.below(3) {
            let path = self.rng.pick(&PATHS);
            let declaration = match self.rng.below(8) {
                0..=2 => {
                    let count = 1 + self.rng.below(3);
                    let names = self.fresh_type_names(count);
                    format!("import type {{ {} }} from \"{path}\"", names.join(", "))
                }
                3 | 4 => {
                    let ty = self.fresh_type_name();
                    let value = self.fresh_function_name();
                    self.top_level.push(value.clone());
                    format!("import {{ type {ty}, {value} }} from \"{path}\"")
                }
                5 => {
                    let ty = self.fresh_type_name();
                    format!("import type {ty} from \"{path}\"")
                }
                6 => {
                    let name = self.fresh_noun();
                    self.top_level.push(name.clone());
                    format!("import {name} = require(\"{path}\")")
                }
                _ => {
                    let name = self.fresh("schema");
                    self.top_level.push(name.clone());
                    format!("import * as {name} from \"{path}\"")
                }
            };
            self.line(&format!("{declaration}{}", self.semi()));
        }
    }

    fn fresh_type_name(&mut self) -> String {
        let noun = capitalize(self.rng.pick(&NOUNS));
        let suffix = self.rng.pick(&TYPE_SUFFIXES);
        let name = self.fresh(&format!("{noun}{suffix}"));
        self.types.push(name.clone());
        name
    }

    fn fresh_type_names(&mut self, count: u64) -> Vec<String> {
        (0..count).map(|_| self.fresh_type_name()).collect()
    }

    /// A top-level declaration that only TypeScript has.
    pub(super) fn ts_top_level_item(&mut self) {
        if self.rng.percent(10) {
            let text = self.rng.pick(&COMMENTS);
            self.line(&format!("/** {text} */"));
        }
        match self.rng.below(14) {
            0..=3 => self.interface_declaration(),
            4..=6 => self.type_alias(),
            7 => self.enum_declaration(""),
            8 => self.namespace_declaration(),
            9 => self.overloads(),
            10 => self.type_guard(),
            11 => self.ambient_declaration(),
            12 if !self.types.is_empty() => {
                let count = 1 + self.rng.below(2);
                let mut names = Vec::new();
                for _ in 0..count {
                    names.push(self.types[self.rng.index(self.types.len())].clone());
                }
                names.sort();
                names.dedup();
                self.line(&format!(
                    "export type {{ {} }}{}",
                    names.join(", "),
                    self.semi()
                ));
            }
            _ => self.interface_declaration(),
        }
    }

    fn interface_declaration(&mut self) {
        let export = self.export_prefix(60);
        let name = self.fresh_type_name();
        let scope = self.type_params.len();
        let generics = if self.rng.percent(25) {
            self.type_parameters(false)
        } else {
            String::new()
        };
        let heritage = if !self.interfaces.is_empty() && self.rng.percent(30) {
            let base = self.interfaces[self.rng.index(self.interfaces.len())].clone();
            format!(" extends {base}")
        } else {
            String::new()
        };
        self.line(&format!("{export}interface {name}{generics}{heritage} {{"));
        self.indent += 1;
        for _ in 0..=self.rng.below(6) {
            let member = self.interface_member(&name);
            self.line(&member);
        }
        self.indent -= 1;
        self.line("}");
        self.type_params.truncate(scope);
        self.interfaces.push(name);
    }

    fn interface_member(&mut self, owner: &str) -> String {
        let semi = if self.rng.percent(85) { ";" } else { "" };
        match self.rng.below(12) {
            0 | 1 => {
                let name = self.rng.pick(&VERBS);
                let mark = if self.rng.percent(15) { "?" } else { "" };
                let scope = self.type_params.len();
                let generics = if self.rng.percent(15) {
                    self.type_parameters(false)
                } else {
                    String::new()
                };
                let parameters = self.signature_parameters(1);
                let result = self.ty(2);
                self.type_params.truncate(scope);
                format!("{name}{mark}{generics}({parameters}): {result}{semi}")
            }
            2 => format!("[key: string]: {}{semi}", self.ty(1)),
            3 => {
                let parameters = self.signature_parameters(1);
                format!("({parameters}): {}{semi}", self.ty(1))
            }
            4 => {
                let parameters = self.signature_parameters(1);
                format!("new ({parameters}): {owner}{semi}")
            }
            5 => format!("readonly {}: {}{semi}", self.fresh_property(), self.ty(2)),
            6 => format!("{}?: {}{semi}", self.fresh_property(), self.ty(2)),
            7 => format!("get {}(): {}{semi}", self.fresh_property(), self.ty(1)),
            _ => format!("{}: {}{semi}", self.fresh_property(), self.ty(2)),
        }
    }

    fn fresh_property(&mut self) -> String {
        let property = self.rng.pick(&PROPERTIES);
        self.fresh(property)
    }

    fn type_alias(&mut self) {
        let export = self.export_prefix(60);
        let name = self.fresh_type_name();
        let scope = self.type_params.len();
        let generics = if self.rng.percent(30) {
            self.type_parameters(false)
        } else {
            String::new()
        };
        let ty = self.ty(3);
        self.type_params.truncate(scope);
        self.line(&format!(
            "{export}type {name}{generics} = {ty}{}",
            self.semi()
        ));
    }

    fn enum_declaration(&mut self, prefix: &str) {
        let export = self.export_prefix(50);
        let kind = self
            .rng
            .pick(&["enum", "enum", "const enum", "declare enum"]);
        let noun = capitalize(self.rng.pick(&NOUNS));
        let name = self.fresh(&format!("{noun}Kind"));
        self.types.push(name.clone());
        self.line(&format!("{prefix}{export}{kind} {name} {{"));
        self.indent += 1;
        let mut members: Vec<String> = Vec::new();
        let string_valued = self.rng.percent(40);
        for _ in 0..=self.rng.below(5) {
            let member = capitalize(&self.fresh_noun());
            let initializer = if string_valued {
                format!(" = \"{}\"", member.to_lowercase())
            } else {
                match self.rng.below(4) {
                    0 => String::new(),
                    1 => format!(" = {}", self.rng.below(100)),
                    2 if !members.is_empty() => {
                        let previous = &members[self.rng.index(members.len())];
                        format!(" = {previous} << 1")
                    }
                    _ => String::new(),
                }
            };
            // A member without a value after a string member would need one.
            let initializer = if initializer.is_empty() && !members.is_empty() && string_valued {
                format!(" = \"{member}\"")
            } else {
                initializer
            };
            self.line(&format!("{member}{initializer},"));
            members.push(member);
        }
        self.indent -= 1;
        self.line("}");
    }

    fn namespace_declaration(&mut self) {
        let export = self.export_prefix(40);
        let noun = capitalize(self.rng.pick(&NOUNS));
        let name = self.fresh(&noun);
        let nested = if self.rng.percent(20) {
            format!(".{}", self.fresh("Internal"))
        } else {
            String::new()
        };
        self.line(&format!("{export}namespace {name}{nested} {{"));
        self.indent += 1;
        let scope = self.locals.len();
        for _ in 0..=self.rng.below(3) {
            match self.rng.below(4) {
                0 => {
                    let constant = self.fresh_noun();
                    let ty = self.ty(1);
                    let value = self.expression(Frame::TOP, 2);
                    self.line(&format!(
                        "export const {constant}: {ty} = {value}{}",
                        self.semi()
                    ));
                    self.locals.push(constant);
                }
                1 => self.function_declaration(Frame::TOP, Export::Maybe, 1),
                2 => self.interface_declaration(),
                _ => self.type_alias(),
            }
        }
        self.locals.truncate(scope);
        self.indent -= 1;
        self.line("}");
        self.top_level.push(name);
    }

    fn overloads(&mut self) {
        let export = self.export_prefix(60);
        let name = self.fresh_function_name();
        let parameter = self.fresh_noun();
        let first = self.ty_atom(1);
        let second = self.ty_atom(1);
        let semi = self.semi();
        let result = self.ty(1);
        self.line(&format!(
            "{export}function {name}({parameter}: {first}): {result};"
        ));
        let result = self.ty(1);
        self.line(&format!(
            "{export}function {name}({parameter}: {second}): {result};"
        ));
        self.line(&format!(
            "{export}function {name}({parameter}: {first} | {second}): unknown {{"
        ));
        self.indent += 1;
        self.line(&format!("return {parameter}{semi}"));
        self.indent -= 1;
        self.line("}");
        self.top_level.push(name);
    }

    fn type_guard(&mut self) {
        let export = self.export_prefix(60);
        let noun = capitalize(self.rng.pick(&NOUNS));
        let ty = if self.types.is_empty() {
            self.ty_atom(1)
        } else {
            self.types[self.rng.index(self.types.len())].clone()
        };
        let semi = self.semi();
        if self.rng.percent(60) {
            let name = self.fresh(&format!("is{noun}"));
            self.line(&format!(
                "{export}function {name}(value: unknown): value is {ty} {{"
            ));
            self.indent += 1;
            self.line(&format!(
                "return typeof value === \"object\" && value !== null{semi}"
            ));
            self.indent -= 1;
            self.line("}");
            self.top_level.push(name);
        } else {
            let name = self.fresh(&format!("assert{noun}"));
            let predicate = if self.rng.percent(70) {
                format!("value is {ty}")
            } else {
                "value".to_owned()
            };
            self.line(&format!(
                "{export}function {name}(value: unknown, message?: string): asserts {predicate} {{"
            ));
            self.indent += 1;
            self.line(&format!("if (!value) throw new Error(message){semi}"));
            self.indent -= 1;
            self.line("}");
            self.top_level.push(name);
        }
    }

    fn ambient_declaration(&mut self) {
        let semi = self.semi();
        match self.rng.below(6) {
            0 => {
                let name = self.fresh_noun();
                let ty = self.ty(2);
                let kind = self.rng.pick(&["const", "let", "var"]);
                self.line(&format!("declare {kind} {name}: {ty}{semi}"));
                self.top_level.push(name);
            }
            1 => {
                let name = self.fresh_function_name();
                let parameters = self.signature_parameters(1);
                let result = self.ty(1);
                self.line(&format!(
                    "declare function {name}({parameters}): {result}{semi}"
                ));
                self.top_level.push(name);
            }
            2 => {
                self.line("declare global {");
                self.indent += 1;
                self.line("interface Window {");
                self.indent += 1;
                let property = self.fresh_property();
                let ty = self.ty(1);
                self.line(&format!("{property}?: {ty};"));
                self.indent -= 1;
                self.line("}");
                self.indent -= 1;
                self.line("}");
            }
            3 => {
                let noun = self.rng.pick(&NOUNS);
                let module = self.fresh(&format!("fictional-{noun}"));
                self.line(&format!("declare module \"{module}\" {{"));
                self.indent += 1;
                let constant = self.fresh_noun();
                let ty = self.ty(1);
                self.line(&format!("export const {constant}: {ty};"));
                let function = self.fresh_function_name();
                let parameters = self.signature_parameters(1);
                self.line(&format!("export function {function}({parameters}): void;"));
                self.indent -= 1;
                self.line("}");
            }
            4 => {
                let extension = self.rng.pick(&["svg", "png", "module.css"]);
                self.line(&format!("declare module \"*.{extension}\" {{"));
                self.indent += 1;
                self.line("const content: string;");
                self.line("export default content;");
                self.indent -= 1;
                self.line("}");
            }
            _ => self.declared_class(),
        }
    }

    fn declared_class(&mut self) {
        let noun = capitalize(self.rng.pick(&NOUNS));
        let name = self.fresh(&format!("{noun}Client"));
        let scope = self.type_params.len();
        let generics = if self.rng.percent(30) {
            self.type_parameters(false)
        } else {
            String::new()
        };
        let export = self.export_prefix(50);
        self.line(&format!("{export}declare class {name}{generics} {{"));
        self.indent += 1;
        let parameters = self.signature_parameters(1);
        self.line(&format!("constructor({parameters});"));
        for _ in 0..=self.rng.below(3) {
            let modifier = self.rng.pick(&["", "", "readonly ", "static ", "private "]);
            let property = self.fresh_property();
            if modifier == "private " {
                self.line(&format!("private {property};"));
            } else {
                let ty = self.ty(1);
                self.line(&format!("{modifier}{property}: {ty};"));
            }
        }
        let method = self.rng.pick(&VERBS);
        let parameters = self.signature_parameters(1);
        let result = self.ty(1);
        self.line(&format!("{method}({parameters}): {result};"));
        self.indent -= 1;
        self.line("}");
        self.type_params.truncate(scope);
        self.types.push(name.clone());
        self.top_level.push(name);
    }

    /// A class with TypeScript members: modifiers, typed fields, parameter
    /// properties, overloads, accessors, generics, decorators, and abstract
    /// members.
    pub(super) fn ts_class_declaration(&mut self) {
        if self.rng.percent(12) {
            let decorator = self.rng.pick(&DECORATORS);
            self.line(&format!("@{decorator}"));
        }
        let export = self.export_prefix(60);
        let is_abstract = self.rng.percent(12);
        let noun = capitalize(self.rng.pick(&NOUNS));
        let suffix = self.rng.pick(&CLASS_SUFFIXES);
        let name = self.fresh(&format!("{noun}{suffix}"));
        let scope = self.type_params.len();
        let generics = if self.rng.percent(25) {
            self.type_parameters(false)
        } else {
            String::new()
        };
        let base = if !self.classes.is_empty() && self.rng.percent(30) {
            Some(self.classes[self.rng.index(self.classes.len())].clone())
        } else {
            None
        };
        let mut heritage = match &base {
            Some(base) => format!(" extends {base}"),
            None => String::new(),
        };
        if !self.interfaces.is_empty() && self.rng.percent(30) {
            let contract = self.interfaces[self.rng.index(self.interfaces.len())].clone();
            heritage.push_str(&format!(" implements {contract}"));
        }
        let keyword = if is_abstract {
            "abstract class"
        } else {
            "class"
        };
        self.line(&format!("{export}{keyword} {name}{generics}{heritage} {{"));
        self.indent += 1;
        let derived = base.is_some();
        let frame = Frame {
            in_class: true,
            ..Frame::TOP
        };
        self.privates.clear();
        let mut private_fields = Vec::new();
        for _ in 0..self.rng.below(3) {
            let noun = self.rng.pick(&NOUNS);
            private_fields.push(self.fresh(&format!("#{noun}")));
        }
        self.privates.extend(private_fields.iter().cloned());
        if self.rng.percent(5) {
            self.line("[key: string]: unknown;");
        }
        for private in private_fields {
            let modifier = self.rng.pick(&["", "", "static ", "readonly "]);
            let ty = self.ty(1);
            let value = self.operand(frame, 1);
            self.line(&format!("{modifier}{private}: {ty} = {value};"));
        }
        for _ in 0..self.rng.below(4) {
            self.class_field(frame, derived);
        }
        if is_abstract {
            let method = self.fresh_function_name();
            let parameters = self.signature_parameters(1);
            let result = self.ty(1);
            let access = self.rng.pick(&["", "protected "]);
            self.line(&format!(
                "{access}abstract {method}({parameters}): {result};"
            ));
        }
        if self.rng.percent(70) {
            self.ts_constructor(frame, derived);
        }
        if self.rng.percent(10) {
            self.method_overloads(frame);
        }
        if self.rng.percent(20) {
            self.accessor_pair(frame);
        }
        for _ in 0..=self.rng.below(3) {
            self.ts_method(frame, derived);
        }
        if self.rng.percent(5) {
            self.line("static {");
            self.body(frame, 1, false);
            self.line("}");
        }
        self.indent -= 1;
        self.line("}");
        self.privates.clear();
        self.type_params.truncate(scope);
        self.classes.push(name.clone());
        self.types.push(name.clone());
        self.top_level.push(name);
    }

    fn class_field(&mut self, frame: Frame, derived: bool) {
        if self.rng.percent(10) {
            let decorator = self.rng.pick(&DECORATORS);
            self.line(&format!("@{decorator}"));
        }
        let mut modifiers = String::new();
        if self.rng.percent(40) {
            modifiers.push_str(self.rng.pick(&ACCESSIBILITY));
            modifiers.push(' ');
        }
        if self.rng.percent(15) {
            modifiers.push_str("static ");
        }
        if derived && self.rng.percent(10) {
            modifiers.push_str("override ");
        }
        if self.rng.percent(25) {
            modifiers.push_str("readonly ");
        }
        let property = self.fresh_property();
        let ty = self.ty(2);
        let line = match self.rng.below(6) {
            0 => format!("{modifiers}{property}?: {ty};"),
            1 => format!("{modifiers}{property}!: {ty};"),
            2 => format!("declare {modifiers}{property}: {ty};"),
            3 => {
                let value = self.expression(frame, 2);
                format!("{modifiers}{property} = {value};")
            }
            _ => {
                let value = self.expression(frame, 2);
                format!("{modifiers}{property}: {ty} = {value};")
            }
        };
        self.line(&line);
    }

    fn ts_constructor(&mut self, frame: Frame, derived: bool) {
        let scope = self.locals.len();
        let parameters = if self.rng.percent(50) {
            let mut parameters = Vec::new();
            for _ in 0..=self.rng.below(3) {
                let name = self.fresh_noun();
                let access = self.rng.pick(&ACCESSIBILITY);
                let readonly = if self.rng.percent(40) {
                    " readonly"
                } else {
                    ""
                };
                let ty = self.ty(1);
                parameters.push(format!("{access}{readonly} {name}: {ty}"));
                self.locals.push(name);
            }
            parameters.join(", ")
        } else {
            self.parameters(frame)
        };
        self.line(&format!("constructor({parameters}) {{"));
        if derived {
            self.indent += 1;
            let arguments = self.arguments(frame, 1);
            self.line(&format!("super({arguments}){}", self.semi()));
            self.indent -= 1;
        }
        self.body(frame.function(false, false), 1, false);
        self.line("}");
        self.locals.truncate(scope);
    }

    fn method_overloads(&mut self, frame: Frame) {
        let name = self.fresh_function_name();
        let parameter = self.fresh_noun();
        let first = self.ty_atom(1);
        let second = self.ty_atom(1);
        self.line(&format!("{name}({parameter}: {first}): {first};"));
        self.line(&format!("{name}({parameter}: {second}): {second};"));
        self.line(&format!(
            "{name}({parameter}: {first} | {second}): {first} | {second} {{"
        ));
        self.locals.push(parameter.clone());
        self.body(frame.function(false, false), 1, false);
        self.locals.pop();
        self.indent += 1;
        self.line(&format!("return {parameter}{}", self.semi()));
        self.indent -= 1;
        self.line("}");
    }

    fn accessor_pair(&mut self, frame: Frame) {
        let property = self.fresh_property();
        let ty = self.ty(1);
        let access = if self.rng.percent(30) { "public " } else { "" };
        self.line(&format!("{access}get {property}(): {ty} {{"));
        self.body(frame.function(false, false), 1, true);
        self.line("}");
        self.line(&format!("{access}set {property}(value: {ty}) {{"));
        self.locals.push("value".to_owned());
        self.body(frame.function(false, false), 1, false);
        self.locals.pop();
        self.line("}");
    }

    fn ts_method(&mut self, frame: Frame, derived: bool) {
        if self.rng.percent(10) {
            let decorator = self.rng.pick(&DECORATORS);
            self.line(&format!("@{decorator}"));
        }
        let mut modifiers = String::new();
        if self.rng.percent(30) {
            modifiers.push_str(self.rng.pick(&ACCESSIBILITY));
            modifiers.push(' ');
        }
        let is_static = self.rng.percent(15);
        if is_static {
            modifiers.push_str("static ");
        }
        if derived && !is_static && self.rng.percent(10) {
            modifiers.push_str("override ");
        }
        let is_async = self.rng.percent(25);
        if is_async {
            modifiers.push_str("async ");
        }
        let name = self.fresh_function_name();
        let scope = self.locals.len();
        let type_scope = self.type_params.len();
        let generics = if self.rng.percent(15) {
            self.type_parameters(false)
        } else {
            String::new()
        };
        let parameters = self.parameters(frame);
        let result = self.return_annotation(is_async, false);
        self.line(&format!(
            "{modifiers}{name}{generics}({parameters}){result} {{"
        ));
        let inner = frame
            .function(is_async, false)
            .with_super(derived && !is_static);
        self.body(inner, 2, true);
        self.line("}");
        self.locals.truncate(scope);
        self.type_params.truncate(type_scope);
    }

    /// The props interface of a TSX component, returning its name.
    pub(super) fn props_interface(&mut self, component: &str, props: &[String]) -> String {
        let name = format!("{component}Props");
        self.types.push(name.clone());
        self.line(&format!("interface {name} {{"));
        self.indent += 1;
        for prop in props {
            let mark = if self.rng.percent(30) { "?" } else { "" };
            let ty = self.ty(2);
            self.line(&format!("{prop}{mark}: {ty};"));
        }
        if self.rng.percent(30) {
            self.line("children?: React.ReactNode;");
        }
        self.indent -= 1;
        self.line("}");
        self.blank();
        name
    }

    /// A typed state hook call in a TSX component.
    pub(super) fn state_hook(&mut self, frame: Frame) {
        let state = self.fresh_noun();
        let setter = self.fresh(&format!("set{}", capitalize(&state)));
        let ty = self.ty(1);
        let initial = self.atom(frame);
        self.line(&format!(
            "const [{state}, {setter}] = useState<{ty}>({initial}){}",
            self.semi()
        ));
        self.locals.push(state);
        self.locals.push(setter);
    }

    /// Generates a declaration file of at least `target` bytes, with only
    /// ambient declarations.
    pub(super) fn declaration_file(&mut self, target: usize) {
        if self.rng.percent(40) {
            self.line("/// <reference types=\"next\" />");
            if self.rng.percent(50) {
                self.line("/// <reference types=\"next/image-types/global\" />");
            }
            self.blank();
        }
        if self.rng.percent(50) {
            let path = self.rng.pick(&PATHS);
            let count = 1 + self.rng.below(2);
            let names = self.fresh_type_names(count);
            self.line(&format!(
                "import type {{ {} }} from \"{path}\"{}",
                names.join(", "),
                self.semi()
            ));
            self.blank();
        }
        while self.out.len() < target {
            self.declaration_item();
            self.blank();
        }
        if self.rng.percent(30) {
            self.line(&format!("export {{}}{}", self.semi()));
        }
    }

    fn declaration_item(&mut self) {
        let semi = self.semi();
        match self.rng.below(12) {
            0..=2 => self.interface_declaration(),
            3 | 4 => self.type_alias(),
            5 => {
                let name = self.fresh_function_name();
                for _ in 0..=self.rng.below(2) {
                    let parameters = self.signature_parameters(2);
                    let result = self.ty(2);
                    self.line(&format!(
                        "export declare function {name}({parameters}): {result}{semi}"
                    ));
                }
            }
            6 => {
                let name = self.fresh_noun();
                let ty = self.ty(2);
                let form = self
                    .rng
                    .pick(&["export const", "export declare const", "export let"]);
                self.line(&format!("{form} {name}: {ty}{semi}"));
            }
            7 | 8 => self.ambient_declaration(),
            9 => {
                self.line("declare global {");
                self.indent += 1;
                self.line("namespace NodeJS {");
                self.indent += 1;
                self.line("interface ProcessEnv {");
                self.indent += 1;
                for _ in 0..=self.rng.below(3) {
                    let noun = self.rng.pick(&NOUNS).to_uppercase();
                    let variable = self.fresh(&format!("NEXT_PUBLIC_{noun}_"));
                    self.line(&format!("readonly {variable}?: string;"));
                }
                self.indent -= 1;
                self.line("}");
                self.indent -= 1;
                self.line("}");
                self.indent -= 1;
                self.line("}");
            }
            10 => self.enum_declaration(""),
            _ => {
                let name = capitalize(&self.fresh_noun());
                self.line(&format!("export declare namespace {name} {{"));
                self.indent += 1;
                let constant = self.fresh_noun();
                let ty = self.ty(1);
                self.line(&format!("const {constant}: {ty};"));
                let function = self.fresh_function_name();
                let parameters = self.signature_parameters(1);
                self.line(&format!("function {function}({parameters}): void;"));
                self.indent -= 1;
                self.line("}");
            }
        }
    }
}
