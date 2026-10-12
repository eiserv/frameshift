//! TypeScript and TSX contracts through the public engine facade.

use frameshift_engine::{
    JsDialect, ParseLimits, SourceGoal, SourceText, SyntaxNodeKind, TextLimits,
    TypeScriptKind as Ts, parse_js_expression, parse_js_program,
};

fn input(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn check(text: &str, dialect: JsDialect) {
    let source = input(text);
    let tree = parse_js_program(&source, SourceGoal::Module, dialect, ParseLimits::default())
        .unwrap_or_else(|error| panic!("{error}: {text}"));
    assert_eq!(source.as_str(), text);
    assert_eq!(source.slice(tree.consumed_span()).unwrap(), text);
    let mut reached = vec![false; tree.nodes().len()];
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        assert!(!reached[id.index()], "shared node: {text}");
        reached[id.index()] = true;
        let parent = tree.node(id).unwrap().span();
        source.slice(parent).unwrap();
        let mut previous = parent.start();
        for child in tree.child_nodes(id).unwrap() {
            let span = tree.node(child).unwrap().span();
            assert!(
                span.start() >= previous && span.end() <= parent.end(),
                "child ordering: {text}, {parent:?}, {span:?}"
            );
            previous = span.end();
            stack.push(child);
        }
    }
    assert!(
        reached.iter().all(|&value| value),
        "unreachable node: {text}"
    );
    let again =
        parse_js_program(&source, SourceGoal::Module, dialect, ParseLimits::default()).unwrap();
    assert_eq!(tree.nodes(), again.nodes());
    assert_eq!(tree.comments(), again.comments());
    for node in tree.nodes() {
        if let SyntaxNodeKind::TypeScript { children: list, .. } = node.kind() {
            assert_eq!(tree.children(list), again.children(list));
        }
    }
}

fn ts(text: &str) {
    check(text, JsDialect::TypeScript);
}

#[test]
fn type_operators_preserve_precedence_and_original_spelling() {
    ts("/* head */ type A<T> = (T | string) & keyof T[]; // tail");
    ts("type Result<T> = T extends readonly (infer U)[] ? U : never;");
    ts("type Result<T> = T extends infer U extends string ? U : never;");
    ts("type Nested<T> = A<B<C<T>>>; type Lookup = Table['name'][number];");
    ts("type Literals = 'a' | 1 | -2 | 3n | -4n | true | false | null | undefined;");
    ts(
        "type Symbols = unique symbol; type Primitive = any | unknown | never | void | object | bigint;",
    );
    ts("type Template<T> = `head-${T & string}-${number}`;");
    ts("type Query = typeof value.member<string>; type Imported = import('pkg').Box<string>;");
    ts("type Imported = typeof import('pkg', { with: { 'resolution-mode': 'import' } }).value;");
    ts("type A = | number | string; type B = & A & object;");
    let source = input("type A = string | number & boolean;");
    let tree = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .unwrap();
    let kinds: Vec<_> = tree
        .nodes()
        .iter()
        .filter_map(|node| {
            if let SyntaxNodeKind::TypeScript { kind, .. } = node.kind() {
                Some(kind)
            } else {
                None
            }
        })
        .collect();
    assert!(kinds.contains(&Ts::Union));
    assert!(kinds.contains(&Ts::Intersection));
}

#[test]
fn tuples_mapped_types_and_signatures_are_structured() {
    ts(
        "type Args = [first: string, second?: number, ...rest: boolean[]]; type Tuple<T> = [T, T?, ...T[]];",
    );
    ts("type Mapping<T> = { -readonly [K in keyof T as `get${K & string}`]+?: T[K] };");
    ts(
        "type F = <const T extends object = {}>(this: Context, value: T, extra?: number, ...rest: T[]) => T;",
    );
    ts("type C = abstract new <T>(value: T) => Box<T>;");
    ts(
        "type O = { readonly name?: string; [key: string]: unknown; <T>(x: T): T; new(x: string): O; method<T>(x: T): x is T; get size(): number; set size(value: number); };",
    );
    ts("type Assert = (x: unknown) => asserts x is string;");
    ts("interface Api<T> extends Base<T>, Other { readonly field: T; call(value: T): this; }");
}

#[test]
fn functions_arrows_and_binding_annotations_share_javascript_bodies() {
    ts("export function identity<T extends object>(value: T): T { return value; }");
    ts(
        "function f(x: string): string; function f(x: number): number; function f(x: unknown): unknown { return x; }",
    );
    ts("const f = <T>(x: T): T => x; const g = async <T>(x: T): Promise<T> => x;");
    ts(
        "const f = (x: number, y?: string): number => x; const g = async (x: number): Promise<number> => { return x; };",
    );
    ts(
        "const { a, b: local }: Shape = value; let answer!: number; for (const entry of items) { const item: Item = entry; }",
    );
    ts("try { work(); } catch (error: unknown) { report(error); }");
    ts(
        "const object = { method<T>(value: T): T { return value; }, get size(): number { return 1; } };",
    );
}

#[test]
fn classes_preserve_generics_modifiers_and_typed_members() {
    ts(
        "abstract class Box<T> extends Base<T> implements Contract<T> { public readonly value!: T; protected optional?: T; abstract get size(): number; abstract method<U>(x: U): T; constructor(public name: string, private value: T) { super(); } static build<U>(x: U): Box<U> { return new Box<U>('a', x); } }",
    );
    ts(
        "class Store { [key: string]: unknown; accessor name: string = 'a'; override method(): void {} }",
    );
    ts(
        "@sealed class Model { @property field: number = 1; @method call(@parameter value: number): number { return value; } }",
    );
}

#[test]
fn namespaces_ambient_declarations_and_enums_keep_runtime_syntax() {
    ts("enum Color { Red, Blue = 'blue', Green = 2 + 1 } const enum Flag { A = 1, B = A << 1 }");
    ts("namespace A.B { export interface Item { name: string } export const value: number = 1; }");
    ts(
        "declare module 'pkg' { export function call(x: string): void; export const value: string; } declare global { interface Window { field: string } }",
    );
    ts("declare const value: string; declare class C<T> { field: T; method(x: T): void; }");
    ts("module Legacy { export type Name = string; } import Alias = Legacy;");
}

#[test]
fn literal_types_can_end_an_arrow_return_type() {
    // Regression: `null`, `true`, or `false` at the end of an arrow
    // function's return type was read as an invalid arrow parameter, so the
    // common `(): T | null => value` failed. Found in the E1 real-world sample.
    ts("const find = (id: string): Item | null => { return null; };");
    ts("const a = (): null => null; const b = (): true => true; const c = (): false | 0 => 0;");
    ts("const pick = flag ? (value): Item | null => value : fallback;");
    check(
        "export const View = (): JSX.Element | null => <main />;",
        JsDialect::Tsx,
    );
    check(
        "const first = <T,>(items: T[]): T | null => items[0] ?? null;",
        JsDialect::Tsx,
    );
    let source = input("const a = (): null => null;");
    let tree = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .unwrap();
    let literal = tree
        .nodes()
        .iter()
        .find(|node| {
            matches!(
                node.kind(),
                SyntaxNodeKind::TypeScript {
                    kind: Ts::Literal,
                    ..
                }
            )
        })
        .unwrap();
    assert_eq!(source.slice(literal.span()).unwrap(), "null");
    // In an expression, a literal before `=>` is still no arrow parameter.
    let error = parse_js_program(
        &input("const a = null => null;"),
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .err()
    .unwrap();
    assert_eq!(
        error.kind,
        frameshift_engine::ParseErrorKind::InvalidArrowParameters
    );
}

#[test]
fn declaration_files_are_an_ambient_typescript_dialect() {
    let declarations = "/// <reference types=\"next\" />\nexport const simple: Redirect[]\nexport const complex: Redirect[]\ndeclare const value: string;\nexport function load(path: string): Promise<string>;\nexport interface Redirect { source: string }\n";
    check(declarations, JsDialect::TypeScriptDeclaration);
    // An ordinary TypeScript file is not ambient, so its `const` needs a value.
    let source = input(declarations);
    let error = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .err()
    .unwrap();
    assert_eq!(
        error.kind,
        frameshift_engine::ParseErrorKind::MissingInitializer
    );
    let tree = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScriptDeclaration,
        ParseLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        tree.node(tree.root()).unwrap().kind(),
        SyntaxNodeKind::Program {
            dialect: JsDialect::TypeScriptDeclaration,
            ..
        }
    ));
    // Declaration files have the TypeScript grammar without JSX.
    check(
        "declare const a: unknown; export const b = <string>a;",
        JsDialect::TypeScriptDeclaration,
    );
    assert!(
        parse_js_program(
            &input("export const c = <div />;"),
            SourceGoal::Module,
            JsDialect::TypeScriptDeclaration,
            ParseLimits::default(),
        )
        .is_err()
    );
}

#[test]
fn redeclaration_early_errors_do_not_apply_to_the_typescript_dialects() {
    // TypeScript declares overloads, merges interfaces, namespaces, and
    // enums, and allows ambient redeclaration, so these dialects report no
    // redeclaration errors; a genuine duplicate is a checker diagnostic.
    // The development-time oracle accepted every case below.
    ts("let merged = 1; let merged = 2; var merged = 3;");
    ts("const repeated = 1; const repeated = 2;");
    ts(
        "function over(x: string): void; function over(x: number): void; function over(x: unknown): void {}",
    );
    ts("interface Shape { a: string } interface Shape { b: number }");
    ts("namespace Merge { export const a = 1 } namespace Merge { export const b = 2 }");
    ts(
        "enum Sized { A } enum Sized { B = 2 } class Base {} namespace Base { export const tag = 1 }",
    );
    ts("declare function amb(): void; function amb(): void {}");
    ts("declare const ambient: string; declare const ambient: string;");
    ts("function params(a: string, a: number) {}");
    // The JavaScript dialects keep the early errors.
    for text in [
        "let double = 1; let double = 2;",
        "{ let block = 1; let block = 2; }",
        "try { work(); } catch (error) { let error = 1; }",
    ] {
        let source = input(text);
        let error = parse_js_program(
            &source,
            SourceGoal::Module,
            JsDialect::JavaScript,
            ParseLimits::default(),
        )
        .err()
        .unwrap_or_else(|| panic!("accepted {text}"));
        assert_eq!(
            error.kind,
            frameshift_engine::ParseErrorKind::DuplicateDeclaration,
            "{text}"
        );
    }
}

#[test]
fn type_only_imports_exports_and_import_equals_remain_distinct() {
    ts(
        "import type { Item as Renamed } from 'pkg'; import { type Item, value, type as name } from 'pkg'; export type { Item }; export { type Renamed, value }; export type * as Types from 'pkg';",
    );
    ts(
        "import type Default from 'pkg'; import type * as Types from 'pkg'; import Loader = require('pkg'); import type Alias = Types.Name; export import Reexport = Loader;",
    );
    ts("export = factory; export as namespace Library;");
    ts(
        "export default interface Named { field: string } export abstract class Base {} export type Alias = string;",
    );
    ts("import type from 'pkg'; export { type as renamed }; import { type as as } from 'pkg';");
}

#[test]
fn assertions_instantiations_and_non_null_are_expression_nodes() {
    ts(
        "const a = value as const; const b = value satisfies Record<string, number>; const c = <Shape>value; const d = value!.field!;",
    );
    ts(
        "const value = call<Map<string, Array<number>>>(input); const bound = call<string>; const made = new Box<string>(); const result = call?.<number>(1); const tagged = tag<string>`value`;",
    );
    ts("const compare = a < b > c; const shift = a >> b; const comparison = a < b >> c;");
    ts("target!.field = value; target! = value; (target as Shape).field = value;");
    let source = input("value satisfies Shape");
    let tree =
        parse_js_expression(&source, 0, JsDialect::TypeScript, ParseLimits::default()).unwrap();
    assert!(matches!(
        tree.node(tree.root()).unwrap().kind(),
        SyntaxNodeKind::TypeScript {
            kind: Ts::Satisfies,
            ..
        }
    ));
}

#[test]
fn tsx_resolves_generic_arrows_and_element_arguments() {
    check(
        "const identity = <T,>(value: T): T => value; const constrained = <T extends object>(value: T) => value; const view = <Box<Item> value={identity<Item>(item)} />;",
        JsDialect::Tsx,
    );
    check(
        "function View<T>({value}: Props<T>): Node { return <Box<T>>{value as T}</Box>; }",
        JsDialect::Tsx,
    );
    check(
        "const f = async <T,>(value: T): Promise<T> => value; const g = <T = unknown>(value: T) => <>{value}</>;",
        JsDialect::Tsx,
    );
    let source = input("const identity = <T>(value: T) => value;");
    assert!(
        parse_js_program(
            &source,
            SourceGoal::Module,
            JsDialect::Tsx,
            ParseLimits::default()
        )
        .is_err()
    );
    assert!(
        parse_js_program(
            &source,
            SourceGoal::Module,
            JsDialect::TypeScript,
            ParseLimits::default()
        )
        .is_ok()
    );
}

#[test]
fn instantiation_followers_and_property_access_match_the_pinned_compiler() {
    // A follower that cannot start an expression, a binary operator, or a
    // line break keeps the type arguments. `as`, `satisfies`, `instanceof`,
    // `in`, and the merged comparison operators count as binary operators.
    ts("const cast = call<string> as unknown; const checked = call<string> satisfies unknown;");
    ts("const boxed = call<string> instanceof Box; const keyed = call<string> in table;");
    ts("const merged = call<string> <= 1; const shifted = call<string> << 2;");
    ts("const compared = a < b > c; const fallen = a < b >> c; const lined = call<string>\n[0];");
    // A `>` keeps the comparison reading, and so do `!`, `+`, `-`, and `{`.
    ts("const summed = call<string> + 1; const negated = call<string> - 1;");
    ts("class Derived extends Namespace.Base<T> {}");
    ts("interface Widened extends First<T>, Second<U> {}");
    // A property access directly on an instantiation expression is rejected.
    for text in [
        "const a = call<string>.field;",
        "call<string>?.field;",
        "new call<string>.field;",
    ] {
        let source = input(text);
        let error = parse_js_program(
            &source,
            SourceGoal::Module,
            JsDialect::TypeScript,
            ParseLimits::default(),
        )
        .err()
        .unwrap_or_else(|| panic!("accepted {text}"));
        assert_eq!(
            error.kind,
            frameshift_engine::ParseErrorKind::InstantiationPropertyAccess,
            "{text}"
        );
    }
    // `!` can start an expression, so `call<string>!` compares and cannot
    // end, exactly as the pinned compiler rejects it.
    let source = input("const a = call<string>!;");
    let error = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .err()
    .unwrap();
    assert_eq!(
        error.kind,
        frameshift_engine::ParseErrorKind::ExpectedExpression
    );
}

fn rejects(text: &str, kind: frameshift_engine::ParseErrorKind) {
    let source = input(text);
    let error = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .err()
    .unwrap_or_else(|| panic!("accepted {text}"));
    assert_eq!(error.kind, kind, "{text}");
}

#[test]
fn generated_corpus_regressions_match_the_pinned_compiler() {
    use frameshift_engine::ParseErrorKind;
    // Regressions found with the generated TypeScript corpus (D-043). Each
    // verdict was compared with the pinned compiler, whose checker accepts
    // the assignments below.
    // TypeScript modifiers may follow `static`, also after a line break.
    ts(
        "class A { static readonly a = 1; static override b = 2; declare private static readonly c: number; }",
    );
    ts("class A { public static override readonly a = 1; static\nreadonly b = 2; }");
    ts("class A { static readonly #a = 1; static m() { return A.#a; } static readonly = 3; }");
    // An object type ends at its own brace inside a template substitution.
    ts("const a = `${new Map<string, { a: T }>()}${x as { [K in keyof T]: T[K] }}`;");
    ts("const a = `${(y: { b: number }) => y}${<{ c: T }>z}`;");
    // A `>` directly followed by `=` is re-scanned into `>=`, so the list
    // is no type argument list.
    ts("const a = f<T>= x; b = \"s\" < b, c >= d;");
    rejects("const a = f<T>== x;", ParseErrorKind::ExpectedExpression);
    // Assertions wrap an assignment target inside parentheses or as a
    // destructuring element, never directly.
    ts("(x as any) = y; (<any>x) = y; (x as any)++; [x as any] = y; ({ a: x as any } = y);");
    rejects("x as any = y;", ParseErrorKind::InvalidAssignmentTarget);
    rejects("<any>x = y;", ParseErrorKind::InvalidAssignmentTarget);
    // `<<` is a shift operator and begins no assertion.
    ts("const a = < <T>() => T>y; f<<T>() => T>(y);");
    rejects(
        "const a = <<T>() => T>y;",
        ParseErrorKind::ExpectedExpression,
    );
}

#[test]
fn malformed_types_never_produce_a_partial_tree() {
    for text in [
        "type A = ;",
        "type A = X<Y;",
        "type A = T extends U ? V;",
        "type A = [x:];",
        "type A = { [K in ]: T };",
        "interface A { field: ; }",
        "function f(x:): void {}",
        "const f = (x: T) =>;",
        "const x = call<T>(;",
        "class C { field: ; }",
        "enum E { A = }",
        "declare module 'pkg' {",
        "type A = `x${T`;",
        "type A = <T>(x: T) T;",
        "const x = <T>;",
        "type A = a + b;",
    ] {
        let source = input(text);
        let one = parse_js_program(
            &source,
            SourceGoal::Module,
            JsDialect::TypeScript,
            ParseLimits::default(),
        )
        .err()
        .unwrap_or_else(|| panic!("accepted {text}"));
        let two = parse_js_program(
            &source,
            SourceGoal::Module,
            JsDialect::TypeScript,
            ParseLimits::default(),
        )
        .err()
        .unwrap();
        assert_eq!(one, two);
        assert!(one.offset <= text.len());
    }
}

#[test]
fn typescript_is_explicit_and_resource_limits_include_speculation() {
    let source = input("type Answer = number;");
    for dialect in [JsDialect::JavaScript, JsDialect::Jsx] {
        assert!(
            parse_js_program(&source, SourceGoal::Module, dialect, ParseLimits::default()).is_err()
        );
    }
    for limits in [
        ParseLimits {
            max_nodes: 1,
            ..ParseLimits::default()
        },
        ParseLimits {
            max_tokens: 1,
            ..ParseLimits::default()
        },
        ParseLimits {
            max_nesting_depth: 1,
            ..ParseLimits::default()
        },
    ] {
        assert!(
            parse_js_program(&source, SourceGoal::Module, JsDialect::TypeScript, limits).is_err()
        );
    }
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            for text in [
                format!("type T = {}number{};", "(".repeat(1_000), ")".repeat(1_000)),
                format!("type T = {}number;", "keyof ".repeat(1_000)),
                format!(
                    "type T = {}number{};",
                    "A<".repeat(1_000),
                    ">".repeat(1_000)
                ),
            ] {
                let source = input(&text);
                let error = parse_js_program(
                    &source,
                    SourceGoal::Module,
                    JsDialect::TypeScript,
                    ParseLimits::default(),
                )
                .err()
                .unwrap();
                assert_eq!(error.kind, frameshift_engine::ParseErrorKind::ResourceLimit);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
