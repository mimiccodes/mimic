//! The catalog of diagnostic codes.
//!
//! Every error the compiler reports carries one of these codes. Codes are
//! **stable**: once published, an id keeps its meaning forever (retired codes
//! are kept, never reused), so tools and agents can match on `code` instead of
//! parsing messages. `mimic explain <code>` prints the long explanation.
//!
//! Ranges:
//! E01xx lexical · E02xx syntax · E03xx names and modules · E04xx types ·
//! E05xx patterns and match · E06xx bindings and closures · E07xx calls and
//! arguments · E08xx C FFI · E09xx project, files and toolchain.

#[derive(Debug, PartialEq, Eq)]
pub struct Code {
    /// Stable id, e.g. `E0401`.
    pub id: &'static str,
    /// Stable kebab-case name, e.g. `type-mismatch`.
    pub name: &'static str,
    /// One-line summary.
    pub title: &'static str,
    /// Long explanation with an example and the usual fix.
    pub explain: &'static str,
}

macro_rules! codes {
    ($( $konst:ident = $id:literal $name:literal $title:literal $explain:literal; )*) => {
        $( pub static $konst: Code = Code { id: $id, name: $name, title: $title, explain: $explain }; )*
        /// Every code, in id order.
        pub static ALL: &[&Code] = &[ $( &$konst ),* ];
    };
}

codes! {
    // ---- E01xx lexical -------------------------------------------------------
    UNEXPECTED_CHAR = "E0101" "unexpected-character" "a character that cannot start any token"
"The lexer found a character that is not part of Mimic's syntax.

    let x = 3 $ 4      // `$` means nothing in Mimic

Glyphs must be ones from the glyph table (`mimic glyphs`); every glyph also
has a word (`⍳` is `iota`) and a digraph (`\\i`) you can type on any keyboard.";

    UNKNOWN_DIGRAPH = "E0102" "unknown-digraph" "a `\\` digraph that is not in the glyph table"
"A backslash starts a digraph, a two-character ASCII spelling of a glyph:
`\\i` is `⍳`, `\\r` is `⌽`. This one is not in the table.

    let xs = \\q 10

Run `mimic glyphs` for every glyph with its word and digraph.";

    BAD_NUMBER = "E0103" "invalid-number" "a malformed or out-of-range number literal"
"The literal cannot be read as a number of its kind: an integer outside the
i64 range, a malformed float or duration, or a `¯` (high minus) that is not
followed by a digit.

    let big = 99999999999999999999   // does not fit in i64
    let t = 1.5.2s                   // not a duration

Use a float (`1e20`) for very large values.";

    UNTERMINATED_STRING = "E0104" "unterminated-string" "a string literal or `{…}` interpolation without its closing quote or brace"
"String literals end on the same line they start, and every `{` in a string
must be closed by `}`.

    print(\"hello)
    print(\"total: {n\")

Close the string with `\"`; write a literal brace as `\\{`.";

    UNKNOWN_ESCAPE = "E0105" "unknown-escape" "an escape sequence that does not exist"
"Strings support `\\n`, `\\t`, `\\r`, `\\0`, `\\\\`, `\\\"`, `\\{` and `\\}`.

    print(\"C:\\path\")     // `\\p` is not an escape

Double the backslash to get a literal one: `\"C:\\\\path\"`.";

    // ---- E02xx syntax --------------------------------------------------------
    UNEXPECTED_TOKEN = "E0201" "unexpected-token" "the parser expected something else here"
"The code does not fit Mimic's grammar at this point. The message says what
was expected and what was found.

    fn add(a, b) { a + }        // expected an expression, found `}`

Look just before the reported position: a missing operand, bracket or
separator is the usual cause.";

    MODULE_HEADER = "E0202" "module-header" "a file without a valid `module <name>` first line"
"The first line of every .mim file names its module, and that header takes no
braces (the rest of the file is the module's body):

    module shapes         // or `§ shapes`

Nested modules inside a file do use braces: `module inner { … }`.";

    UNSUPPORTED_SYNTAX = "E0203" "unsupported-syntax" "syntax that is reserved but not implemented yet"
"The construct is part of Mimic's design but this compiler version does not
support it yet (for example `pub use` re-exports, or an `extern` ABI other
than \"C\"). The message names the construct; see docs/roadmap.md.";

    NAME_CASE = "E0204" "name-case" "a type or variant name that does not start with an uppercase letter"
"Types and variants start with an uppercase letter; functions, variables and
modules start with a lowercase one. This is how `Circle(r)` (a variant) is
told apart from `circle(r)` (a call) everywhere.

    type shape = circle(f64) | dot      // error
    type Shape = Circle(f64) | Dot      // ok

The suggested fix renames the declaration.";

    DUPLICATE_ENTRY = "E0205" "duplicate-entry" "a field, variant or key listed twice"
"Each field of a record, variant of a union, field in a record literal and key
in a config table may appear only once.

    type Point = { x: f64, x: f64 }
    let p = Point { x: 1.0, x: 2.0 }

Remove or rename the repeated entry.";

    PARAM_RULES = "E0206" "parameter-rules" "a parameter list that breaks Mimic's rules"
"Parameters are immutable, and parameters with a default value come after
those without one.

    fn f(mut x: i64) { … }                 // error: parameters are immutable
    fn g(a = 1, b) { … }                    // error: default before required

To change a parameter, bind a copy: `let mut y = x`.";

    BAD_LET = "E0207" "malformed-let" "a `let` binding with missing or misplaced parts"
"A binding is `let name = value`, `let mut name = value` (or `mut name ← value`),
`let (a, b) = tuple`, or `let name = option else { return … }`.
`let … else` unwraps a value and cannot be `mut`.";

    BAD_ASSIGN_TARGET = "E0208" "invalid-assignment-target" "assigning to something that is not a variable or a place inside one (`xs[i]`, `g[r][c]`, `s.pos.x`)"
"Only `let mut` variables and their elements can be assigned:

    xs[2] = 7        // ok if `xs` is `let mut`
    f(x) = 3         // error

Values are immutable; build a new value with `with` for records.";

    BAD_PIPELINE = "E0209" "invalid-pipeline-stage" "a pipeline stage or `¨` operand that is not a function"
"Each stage after `▷` (`|>`) and the operand of `¨` (`each`) must be a
primitive, a named function or a lambda:

    xs ▷ +/ ▷ 3            // `3` is not a function
    xs ▷ +/ ▷ \\x -> x * 3  // ok";

    CHAINED_COMPARISON = "E0210" "chained-comparison" "comparisons written as a chain"
"`a < b < c` does not mean what it looks like, so Mimic rejects it.

    if 0 < x < 10 { … }            // error
    if 0 < x and x < 10 { … }      // ok";

    BAD_ROUTE = "E0211" "invalid-route-pattern" "a malformed route pattern in `match`"
"Route patterns match strings with typed captures:

    \"/users/{id: i64}\" => …
    \"/posts/{slug}\"    => …        // str capture

Captures are `{name}` or `{name: T}` with T one of str, i64, f64, and two
captures must be separated by literal text.";

    EMPTY_MATCH = "E0212" "empty-match" "a `match` without arms"
"A `match` needs at least one arm (`pattern => value`).";

    BAD_INTERPOLATION = "E0213" "invalid-interpolation" "the expression inside `{…}` in a string does not parse"
"String interpolation evaluates the expression between braces:

    print(\"{name} has {n + 1} items\")

The message shows the error from the embedded expression. Write `\\{` for a
literal brace.";

    PRIMITIVE_AS_NAME = "E0214" "primitive-as-name" "a built-in primitive's word used as a name"
"Every primitive has a reserved word (`sum`, `iota`, `reverse`, …). These
cannot name variables, parameters or functions.

    let sum = 3        // error: `sum` is `+/`

Pick another name (`total`). Run `mimic glyphs` for the reserved words.";

    PRIM_USAGE = "E0215" "primitive-usage" "a primitive used with the wrong form"
"Dyadic primitives take a left and a right argument: write `xs ↑ 3` or
`take(xs, 3)`, not `↑ xs`. Monadic ones take one: `⌽ xs` or `reverse(xs)`.";

    BREAK_OUTSIDE_LOOP = "E0216" "break-outside-loop" "`break` outside a loop"
"`break` (`↯`) leaves the innermost `while` or `for` loop of the function it
is written in. Outside a loop, and inside a lambda (a function of its own,
even when the lambda sits in a loop), there is nothing to leave: use
`return` to leave a function, or `settle` with a `done` test to stop a loop
written as a step.";

    BAD_SIZED_LIST = "E0217" "sized-list" "a sized list `[T × n]` with a wrong length"
"A sized list's type says how many elements it holds: `[i64 × 9]` is nine
integers, `[f64 × 3]` three floats. The length is a whole number of at least
one. A list written out with another count, or an index the compiler can see
is outside it (`p[3]` on `[f64 × 3]`), is refused when checking; a list whose
length is known only when running is checked when it is bound.";

    // ---- E03xx names and modules ---------------------------------------------
    DUPLICATE_DEFINITION = "E0301" "duplicate-definition" "two items with the same name in one module"
"Within one module, functions, types, variants, sub-modules and `use` imports
share one namespace and each name is defined once.

    fn area(s) { … }
    fn area(s) { … }     // error

Rename one of them, or move it into its own module.";

    SHADOWING = "E0302" "shadowing" "a name that hides another one"
"Mimic never lets a name hide another: not a local variable inside the same
function, and not a function in an enclosing module. This keeps every name
meaning one thing in one place.

    let x = 1
    let x = 2         // error

Use `let mut x` and reassign, or pick a different name.";

    UNRESOLVED_NAME = "E0303" "unresolved-name" "a name or path that does not refer to anything"
"Nothing with this name is visible here. Names are looked up in the local
scope, then the current module and its parents, then dependencies (by package
name) and the prelude.

    print(totl)          // did you mean `total`?
    shapes::areaa(s)

Check the spelling (the suggested fix offers the closest name) and the path.";

    PRIVATE_ITEM = "E0304" "private-item" "using an item that is not `pub` from outside its module"
"Items are private to their module (and its sub-modules) unless declared `pub`.

    module shapes { fn area(s) { … } }
    shapes::area(s)      // error: `area` is private

Add `pub` to the definition (the suggested fix does this) if it is meant to
be used from outside.";

    WRONG_KIND = "E0305" "wrong-kind-of-item" "a name used as something it is not"
"The name exists but is the wrong kind of item for this position: a module or
type used as a value, a variable called like a function, a variant used where a
record type is expected, and so on. The message says what the name is.";

    NO_MAIN = "E0306" "missing-main" "a binary without a valid `fn main()`"
"A binary project starts at `fn main()` in `module main` (src/main.mim).
`main` takes no parameters; read the command line with the standard library.

    module main
    fn main() { print(\"hello\") }";

    BAD_PATH = "E0307" "invalid-path" "a path that goes nowhere: `super` above the root or a `use` cycle"
"`super` refers to the parent module and cannot go above the package root;
`use` imports cannot refer to each other in a cycle.";

    // ---- E04xx types ---------------------------------------------------------
    TYPE_MISMATCH = "E0401" "type-mismatch" "a value of the wrong type"
"The value's type does not match what this position requires: a parameter or
return annotation, a record field, a variant payload or a pattern.

    fn half(x: f64) -> f64 { x / 2 }
    half(\"ten\")                // expected f64, found str

Numbers convert where it is lossless (i64 → f64; bool → i64). The message says
what was expected and what was found; a secondary label points at the
declaration that set the expectation.";

    UNKNOWN_TYPE = "E0402" "unknown-type" "a type name that does not exist"
"Built-in types are i64, f64, bool, char, str and any, arrays `[T]`, and tuples
`(T, U)`. Records and unions are declared with `type`.

    fn f(x: int) { … }        // did you mean `i64`?";

    NO_FIELD = "E0403" "no-such-field" "a field or tuple element that does not exist"
"The record has no field with this name, or the tuple has fewer elements.

    type Item = { name: str, price: f64 }
    item.prize              // did you mean `price`?
    pair.2                  // a pair has elements .0 and .1";

    UNION_NOT_RECORD = "E0404" "union-not-record" "record syntax used on a union type"
"Unions have variants, not fields. Construct one of the variants
(`Circle(1.0)`), and read their contents with `match`:

    match s { Circle(r) => r, Rect(w, h) => w * h }";

    TUPLE_ARITY = "E0405" "tuple-arity" "destructuring a tuple with the wrong number of names"
"`let (a, b) = t` needs `t` to be a tuple with exactly as many elements as
names on the left.";

    LET_ELSE_TYPE = "E0406" "let-else-type" "`let … else` or `e else { … }` on a value that is not Option or Result"
"`let x = v else { … }` and `v else { … }` unwrap `Some(x)` or `Ok(x)` and
run the `else` block for `None` or `Err`. Other values cannot be unwrapped
this way; use `match`.";

    NO_OPERATOR = "E0407" "no-operator" "an operator applied to a type that does not support it"
"Arithmetic and comparison work on numbers and arrays of numbers (elementwise).
Records and variants have no operators; compare them with `match` or by
field.";

    TYPE_NOT_KNOWN = "E0408" "type-not-known" "an operation that needs a type the compiler cannot see here"
"`with` copies a record with some fields replaced, so the compiler must know
the record's type at this point. Annotate the variable or parameter:

    fn bump(it: Item) -> Item { it with { qty: it.qty + 1 } }";

    MISSING_FIELDS = "E0409" "missing-fields" "a record literal without all of its fields"
"Every field of a record must be given when it is constructed (fields with
defaults are planned). The message lists the missing ones.";

    UNSUPPORTED_TYPE = "E0410" "unsupported-type" "a type this compiler version cannot represent yet"
"The type is part of the design but not implemented yet (for example arrays
of some element kinds in annotations). The message names the limitation.";

    TYPE_ARGS = "E0411" "type-args" "a generic type used with the wrong number of type arguments"
"A generic type takes exactly as many type arguments as it declares
parameters:

    type Pair[A, B] = { first: A, second: B }
    let p: Pair[i64] = …          // error: Pair takes 2 type arguments

Written without arguments (`Pair`), a generic type means any instance.";

    // ---- E05xx patterns and match ----------------------------------------------
    NON_EXHAUSTIVE = "E0501" "non-exhaustive-match" "a `match` that does not cover every case"
"Every possible value must be handled. For unions, every variant needs an arm
(or a final `_ => …`); for numbers, strings and routes a final `_ => …` is
required.

    match s { Circle(r) => 1, Dot => 0 }     // missing `Rect(…)`

The suggested fix inserts the missing arms with `…` placeholders.";

    PATTERN_ARITY = "E0502" "pattern-arity" "a variant pattern with the wrong number of fields"
"A pattern binds exactly the fields the variant has:

    type Shape = Rect(f64, f64)
    match s { Rect(w) => w }       // error: Rect has 2 fields";

    MIXED_ARMS = "E0503" "mixed-arms" "arms of one `match` matching different union types"
"All variant patterns in one `match` must belong to the same union type.";

    ELSE_MUST_RETURN = "E0504" "else-must-return" "an `else` block that neither gives a value nor returns"
"The `else` block runs when there is no value inside (`None`, `Err`), so it
stands in for that value: it gives one of the same type, or leaves the
function:

    let port = parse_int(text) else { 8080 }
    let user = find(id) else { return Err(\"not found\") }";

    OR_BINDINGS = "E0505" "or-bindings" "alternatives of a `|` pattern binding different names"
"Whichever alternative matches, the arm's body sees the same variables, so
every alternative must bind the same names:

    match s { Circle(r) | Square(r) => r, _ => 0.0 }    // fine
    match s { Circle(r) | Dot => r, _ => 0.0 }          // error: Dot binds no `r`

Inside a larger pattern (`Some(A(x) | B(x))`) alternatives cannot bind names
yet; split them into separate arms.";

    RECORD_PATTERN = "E0506" "record-pattern" "a record pattern that names a missing field or omits fields without `..`"
"A record pattern lists fields by name. Fields it does not mention must be
waived explicitly with `..`:

    match p { Point { x: 0, y } => y, Point { x, .. } => x }";

    // ---- E06xx bindings and closures -----------------------------------------------
    ASSIGN_IMMUTABLE = "E0601" "assign-immutable" "assigning to a variable that is not `mut`"
"Bindings are immutable unless declared `mut`.

    let total = 0
    total = total + 1        // error

The suggested fix declares it `let mut total = 0`.";

    ASSIGN_UNDEFINED = "E0602" "assign-undefined" "assigning to a variable that was never declared"
"Assignment changes an existing `let mut` variable; declare new ones with
`let`:

    count = 1          // error
    let mut count = 1  // ok";

    CAPTURE_MUTATION = "E0603" "capture-mutation" "a lambda modifying a variable of its enclosing function"
"Lambdas can run in parallel, so they may not modify variables they capture.
Return the new value from the lambda instead (`¨` collects the results).";

    LAMBDA_LIMIT = "E0604" "lambda-limit" "a lambda feature not supported yet"
"Lambdas take any number of parameters (`x => …`, `(a, b) => …`) and capture
the variables they use by value. What is not supported yet is described in
the message (for example passing a Mimic function to C as a callback).";

    IMPURE_IN_DATA = "E0605" "effectful-function-in-data" "a function with side effects stored in a value"
"Functions kept inside records, variants, lists or maps must not have side
effects (printing, files, networking, …). Code that later takes them out may
run in parallel, so their effects could happen at any time and in any order.

    ui::input(s.draft, on_input: t => { print(t); Typed(t) })   // error
    ui::input(s.draft, on_input: Typed)                         // ok

Do the effect in the code that handles the message instead.";

    // ---- E07xx calls and arguments ----------------------------------------------
    ARG_COUNT = "E0701" "argument-count" "a call with the wrong number of arguments"
"The function, variant or primitive takes a different number of arguments.

    fn add(a, b) { a + b }
    add(1)           // error: takes 2 arguments, got 1";

    UNKNOWN_PARAM = "E0702" "unknown-parameter" "a named argument that matches no parameter"
"Named arguments use the parameter names of the function being called:

    fn greet(name, greeting = \"hi\") { … }
    greet(name: \"ada\", greting: \"yo\")      // did you mean `greeting`?";

    DUPLICATE_ARG = "E0703" "duplicate-argument" "the same argument given twice"
"Each parameter receives exactly one value, either by position or by name.";

    POSITIONAL_AFTER_NAMED = "E0704" "positional-after-named" "a positional argument after a named one"
"No longer reported. Named arguments take their parameters, and positional
arguments fill the remaining parameters in order, wherever they are written:

    f(1, b: 2)      // a = 1, b = 2
    f(b: 2, 1)      // the same";

    MISSING_ARG = "E0705" "missing-argument" "a required argument that was not given"
"Every parameter without a default value needs an argument.";

    NAMED_ARG_PLACE = "E0706" "named-argument-misplaced" "`name: value` outside a function call"
"Named arguments are only allowed in calls to Mimic functions (not variants,
primitives or extern functions).";

    DEFAULT_NOT_CONST = "E0707" "default-not-constant" "a parameter default that is not a constant"
"Default values are constants (numbers, strings, booleans, variants without
fields, or arrays of those), evaluated where the call is made.";

    // ---- E08xx C FFI ---------------------------------------------------------
    UNSAFE_REQUIRED = "E0801" "unsafe-required" "calling C without `unsafe`"
"C functions can break memory safety, so calls to them go in an `unsafe` block:

    extern \"C\" { fn strlen(s: cstr) -> usize }
    let n = unsafe { strlen(\"hi\") }

If the C functions are memory safe (like libm's `sqrt`), mark the block
`trusted extern \"C\" { … }` and call them directly. The suggested fix wraps the
call in `unsafe { … }`.";

    NOT_C_TYPE = "E0802" "not-a-c-type" "an `extern` signature using a type C does not have"
"Parameters and returns of `extern` functions use C types: i8 … i64, u8 … u64,
isize, usize, f32, f64, bool, c_char, cstr and `ptr[T]`. Arrays cross as
`ptr[T]` plus a length.";

    FFI_UNSUPPORTED = "E0803" "ffi-unsupported" "an FFI feature not supported yet"
"Returning raw pointers from C and passing extern functions as values are not
supported in this version. Full C interop (structs, callbacks, header import)
is Phase B of the roadmap.";

    PTR_OUTSIDE_EXTERN = "E0804" "ptr-outside-extern" "`ptr[T]` used outside an `extern` declaration"
"Raw pointers exist only at the C boundary. Mimic code uses arrays, which the
compiler passes to C as pointers.";

    CIMPORT = "E0805" "c-import-failed" "a C header in `extern \"C\" include` could not be read"
"`include \"header.h\"` reads the header with the embedded Clang, for the
target being built. The header is looked for next to the source file, in the
project root and in `ffi { include = [...] }` of mim.mim, then among the
target's system headers (`<header.h>`). The message contains Clang's error,
for example a missing file or a syntax error in the header.";

    // ---- E09xx project, files and toolchain ----------------------------------
    PROJECT = "E0901" "project-config" "an error in mim.mim, build.mim or the project layout"
"The project files could not be read as a valid project: a malformed or
missing block in mim.mim or build.mim, a wrong value type, or a missing
source directory. See docs/manifest.md.";

    IO = "E0902" "io-error" "a file that could not be read or written"
"The operating system refused a file operation. The message includes the path
and the OS error.";

    ENTRY_MODULE = "E0903" "entry-module" "the wrong top-level module for this kind of build"
"`mimic run file.mim` needs the file to declare `module main`; a library
project needs src/lib.mim declaring `module lib`.";

    DEPENDENCY = "E0904" "dependency" "a dependency that cannot be resolved"
"A dependency in mim.mim could not be found, is not a library, or its mim.mim
names a different package. Check `dependencies { … }` and run `mimic list`.";

    BUILD = "E0905" "build-failed" "code generation, the C runtime or linking failed"
"The program checked cleanly but a later stage failed: LLVM code generation,
compiling the runtime, or linking (for example a missing C library named in
`ffi { link = [...] }`, or a cross target without a C toolchain). The message
contains the tool's output.";
}

/// Look a code up by id (`E0401`, `e0401`, `0401`, `401`) or name (`type-mismatch`).
pub fn find(q: &str) -> Option<&'static Code> {
    let q = q.trim();
    let digits = q.trim_start_matches(['E', 'e']);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        let n: u32 = digits.parse().ok()?;
        return ALL.iter().copied().find(|c| c.id[1..].parse::<u32>().ok() == Some(n));
    }
    ALL.iter().copied().find(|c| c.name.eq_ignore_ascii_case(q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_are_unique_and_well_formed() {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for c in ALL {
            assert!(c.id.len() == 5 && c.id.starts_with('E') && c.id[1..].chars().all(|d| d.is_ascii_digit()), "{}", c.id);
            assert!(c.name.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-'), "{}", c.name);
            assert!(ids.insert(c.id), "duplicate id {}", c.id);
            assert!(names.insert(c.name), "duplicate name {}", c.name);
            assert!(!c.explain.is_empty());
        }
        let v: Vec<_> = ALL.iter().map(|c| c.id).collect();
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(v, sorted, "codes must be listed in id order");
    }

    #[test]
    fn lookup() {
        assert_eq!(find("E0401").unwrap().name, "type-mismatch");
        assert_eq!(find("e401").unwrap().id, "E0401");
        assert_eq!(find("type-mismatch").unwrap().id, "E0401");
        assert!(find("E9999").is_none());
    }
}
