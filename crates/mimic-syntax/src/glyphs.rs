//! The glyph table: single source of truth for glyph ↔ word ↔ ASCII digraph.
//!
//! Every primitive and every glyph-able piece of syntax appears here exactly
//! once. The lexer, `mimic glyphs`, the docs and the runtime opcode numbering
//! are all derived from this table.

/// Array primitives. The discriminant is the runtime opcode (see runtime/mimic_rt.h).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prim {
    Iota = 0,
    Shape = 1,
    Reverse = 2,
    Sum = 3,
    Product = 4,
    Maximum = 5,
    Minimum = 6,
    Len = 7,
    Print = 8,
    Take = 9,
    Drop = 10,
    Keep = 11,
    Each = 12,
    Range = 13,
    /// Lowered by the parser to binary/unary operators (never reach the runtime as prims).
    Max = 14,
    Min = 15,
    Abs = 16,
    /// Loop words: higher-order, with function operands (never runtime prims).
    Gather = 17,
    Trail = 18,
    Evolve = 19,
    Settle = 20,
    /// Arrays of rank ≥ 2.
    Reshape = 21,
    Transpose = 22,
    MatMul = 23,
}

pub struct PrimInfo {
    pub prim: Prim,
    pub glyph: &'static str,
    pub word: &'static str,
    /// Backslash digraph for typing the glyph on an ASCII keyboard (`\i` → `⍳`).
    pub digraph: Option<&'static str>,
    /// ASCII spelling that the lexer also accepts (e.g. `*/` for `×/`).
    pub ascii: Option<&'static str>,
    pub arity: usize,
    pub doc: &'static str,
}

pub const PRIMS: &[PrimInfo] = &[
    PrimInfo { prim: Prim::Iota, glyph: "⍳", word: "iota", digraph: Some("\\i"), ascii: None, arity: 1, doc: "⍳ n → [0 1 … n-1]" },
    PrimInfo { prim: Prim::Shape, glyph: "⍴", word: "shape", digraph: Some("\\r"), ascii: None, arity: 1, doc: "shape of an array ([] for scalars)" },
    PrimInfo { prim: Prim::Reverse, glyph: "⌽", word: "reverse", digraph: Some("\\o"), ascii: None, arity: 1, doc: "reverse a vector" },
    PrimInfo { prim: Prim::Sum, glyph: "+/", word: "sum", digraph: None, ascii: None, arity: 1, doc: "sum reduction" },
    PrimInfo { prim: Prim::Product, glyph: "×/", word: "product", digraph: None, ascii: Some("*/"), arity: 1, doc: "product reduction" },
    PrimInfo { prim: Prim::Maximum, glyph: "⌈/", word: "maximum", digraph: Some("\\M"), ascii: None, arity: 1, doc: "maximum reduction" },
    PrimInfo { prim: Prim::Minimum, glyph: "⌊/", word: "minimum", digraph: Some("\\m"), ascii: None, arity: 1, doc: "minimum reduction" },
    PrimInfo { prim: Prim::Len, glyph: "≢", word: "len", digraph: Some("\\#"), ascii: None, arity: 1, doc: "length (tally) of the leading axis" },
    PrimInfo { prim: Prim::Print, glyph: "⎕", word: "print", digraph: Some("\\p"), ascii: None, arity: 1, doc: "print a value followed by a newline" },
    PrimInfo { prim: Prim::Take, glyph: "↑", word: "take", digraph: Some("\\^"), ascii: None, arity: 2, doc: "xs ↑ n → first n (last n if n<0)" },
    PrimInfo { prim: Prim::Drop, glyph: "↓", word: "drop", digraph: Some("\\v"), ascii: None, arity: 2, doc: "xs ↓ n → drop first n (last n if n<0)" },
    PrimInfo { prim: Prim::Keep, glyph: "⌿", word: "keep", digraph: Some("\\k"), ascii: None, arity: 2, doc: "xs ⌿ counts → replicate/filter by mask" },
    PrimInfo { prim: Prim::Each, glyph: "¨", word: "each", digraph: Some("\\e"), ascii: None, arity: 2, doc: "xs ¨ f → apply f to every element" },
    PrimInfo { prim: Prim::Range, glyph: "‥", word: "range", digraph: Some("\\."), ascii: None, arity: 2, doc: "lo ‥ hi → [lo lo+1 … hi-1]" },
    PrimInfo { prim: Prim::Max, glyph: "⌈", word: "max", digraph: Some("\\u"), ascii: None, arity: 2, doc: "a ⌈ b → larger of a and b (elementwise)" },
    PrimInfo { prim: Prim::Min, glyph: "⌊", word: "min", digraph: Some("\\d"), ascii: None, arity: 2, doc: "a ⌊ b → smaller of a and b (elementwise)" },
    PrimInfo { prim: Prim::Abs, glyph: "∣", word: "abs", digraph: Some("\\a"), ascii: None, arity: 1, doc: "∣ x → absolute value (elementwise)" },
    PrimInfo { prim: Prim::Gather, glyph: "∕", word: "gather", digraph: Some("\\g"), ascii: None, arity: 2, doc: "xs ▷ gather(f) → combine everything with f (parallel tree; f associative)" },
    PrimInfo { prim: Prim::Trail, glyph: "⍀", word: "trail", digraph: Some("\\t"), ascii: None, arity: 2, doc: "xs ▷ trail(f) → running results of f (parallel prefix; f associative)" },
    PrimInfo { prim: Prim::Evolve, glyph: "⍣", word: "evolve", digraph: Some("\\P"), ascii: None, arity: 3, doc: "x ▷ evolve(n, f) → apply f to the state n times" },
    PrimInfo { prim: Prim::Settle, glyph: "⍥", word: "settle", digraph: Some("\\s"), ascii: None, arity: 3, doc: "x ▷ settle(f, done) → apply f until done(x) holds" },
    PrimInfo { prim: Prim::Reshape, glyph: "⥊", word: "reshape", digraph: Some("\\R"), ascii: None, arity: 2, doc: "xs ⥊ [2 3] → the elements of xs as a 2×3 matrix (repeated to fill)" },
    PrimInfo { prim: Prim::Transpose, glyph: "⍉", word: "transpose", digraph: Some("\\T"), ascii: None, arity: 1, doc: "⍉ m → rows become columns (reverses the axes)" },
    PrimInfo { prim: Prim::MatMul, glyph: "⊠", word: "matmul", digraph: Some("\\X"), ascii: None, arity: 2, doc: "a ⊠ b → matrix product (vectors act as a row or a column)" },
];

impl Prim {
    pub fn info(self) -> &'static PrimInfo {
        PRIMS.iter().find(|p| p.prim == self).expect("prim in table")
    }
    pub fn arity(self) -> usize {
        self.info().arity
    }
    pub fn from_word(w: &str) -> Option<Prim> {
        PRIMS.iter().find(|p| p.word == w).map(|p| p.prim)
    }
}

/// Glyph spellings of syntax (keywords and operators).
pub struct SyntaxGlyph {
    pub glyph: &'static str,
    pub word: &'static str,
    pub digraph: Option<&'static str>,
    pub doc: &'static str,
}

pub const SYNTAX_GLYPHS: &[SyntaxGlyph] = &[
    SyntaxGlyph { glyph: "§", word: "module", digraph: Some("\\S"), doc: "module declaration" },
    SyntaxGlyph { glyph: "ƒ", word: "fn", digraph: Some("\\f"), doc: "function declaration" },
    SyntaxGlyph { glyph: "←", word: "let … =", digraph: Some("\\<"), doc: "x ← 5  ≡  let x = 5" },
    SyntaxGlyph { glyph: "↩", word: "=", digraph: Some("\\="), doc: "x ↩ x + 1  (reassign a mut binding)" },
    SyntaxGlyph { glyph: "▷", word: "|>", digraph: Some("\\>"), doc: "left-to-right pipeline" },
    SyntaxGlyph { glyph: "→", word: "->", digraph: None, doc: "return type" },
    SyntaxGlyph { glyph: "⇒", word: "=>", digraph: None, doc: "lambda arrow" },
    SyntaxGlyph { glyph: "×", word: "*", digraph: Some("\\x"), doc: "multiply" },
    SyntaxGlyph { glyph: "÷", word: "/", digraph: Some("\\/"), doc: "divide (always f64)" },
    SyntaxGlyph { glyph: "⫽", word: "div", digraph: Some("\\%"), doc: "integer division, floored (7 div 2 = 3, ¯7 div 2 = ¯4)" },
    SyntaxGlyph { glyph: "≠", word: "!=", digraph: None, doc: "not equal" },
    SyntaxGlyph { glyph: "≤", word: "<=", digraph: None, doc: "less or equal" },
    SyntaxGlyph { glyph: "≥", word: ">=", digraph: None, doc: "greater or equal" },
    SyntaxGlyph { glyph: "∧", word: "and", digraph: Some("\\&"), doc: "logical and (elementwise)" },
    SyntaxGlyph { glyph: "∨", word: "or", digraph: Some("\\|"), doc: "logical or (elementwise)" },
    SyntaxGlyph { glyph: "¬", word: "not", digraph: Some("\\!"), doc: "logical not (elementwise)" },
    SyntaxGlyph { glyph: "¯", word: "-", digraph: Some("\\_"), doc: "high minus for negative literals: ¯3" },
    SyntaxGlyph { glyph: "⍝", word: "//", digraph: None, doc: "line comment" },
    SyntaxGlyph { glyph: "⋔", word: "arms", digraph: Some("\\y"), doc: "arms (a = …) { … arm(x) … arm(y) }: parallel divide and conquer" },
    SyntaxGlyph { glyph: "⍬", word: "[] of i64", digraph: Some("\\z"), doc: "an empty list of i64; ⍬[f64], ⍬[str], ⍬[Point]: of that element type" },
    SyntaxGlyph { glyph: "⇑", word: "pub", digraph: Some("\\U"), doc: "public: visible outside its module (⇑ ƒ area(…))" },
    SyntaxGlyph { glyph: "⇄", word: "mut", digraph: Some("\\D"), doc: "mutable binding (⇄ x ← 0)" },
    SyntaxGlyph { glyph: "⍰", word: "if", digraph: Some("\\?"), doc: "if (⍰ x > 0 { … } ⋄ { … })" },
    SyntaxGlyph { glyph: "⋄", word: "else", digraph: Some("\\:"), doc: "else, and `e ⋄ { … }`: the value inside an Option or Result, else the block" },
    SyntaxGlyph { glyph: "⏎", word: "return", digraph: Some("\\L"), doc: "return from the function" },
    SyntaxGlyph { glyph: "⇐", word: "use", digraph: Some("\\I"), doc: "use a module (⇐ text)" },
    SyntaxGlyph { glyph: "≔", word: "as", digraph: Some("\\A"), doc: "rename in `use` (⇐ text ≔ t)" },
    SyntaxGlyph { glyph: "⌁", word: "extern", digraph: Some("\\E"), doc: "functions from C (⌁ \"C\" { … })" },
    SyntaxGlyph { glyph: "☡", word: "unsafe", digraph: Some("\\W"), doc: "unsafe block: calls into C" },
    SyntaxGlyph { glyph: "✓", word: "trusted", digraph: Some("\\V"), doc: "trusted extern: C calls outside `unsafe`" },
    SyntaxGlyph { glyph: "⊡", word: "type", digraph: Some("\\Y"), doc: "type declaration (⊡ Point = { x: f64, y: f64 })" },
    SyntaxGlyph { glyph: "⊨", word: "match", digraph: Some("\\K"), doc: "match on patterns" },
    SyntaxGlyph { glyph: "⊕", word: "with", digraph: Some("\\w"), doc: "{ r ⊕ f: x }: a record with fields replaced" },
    SyntaxGlyph { glyph: "●", word: "true", digraph: Some("\\1"), doc: "true" },
    SyntaxGlyph { glyph: "○", word: "false", digraph: Some("\\0"), doc: "false" },
    SyntaxGlyph { glyph: "⊢", word: "test", digraph: Some("\\H"), doc: "a test block (⊢ \"adds\" { expect(1 + 1 == 2) })" },
    SyntaxGlyph { glyph: "∀", word: "for", digraph: Some("\\F"), doc: "a property test's inputs (⊢ \"…\" ∀ x: i64 { … })" },
    SyntaxGlyph { glyph: "⎘", word: "include", digraph: Some("\\C"), doc: "a C header in an extern block (⎘ \"math.h\")" },
];

/// Expand a backslash digraph (`\i`) to its glyph.
pub fn digraph(d: &str) -> Option<&'static str> {
    PRIMS
        .iter()
        .find(|p| p.digraph == Some(d))
        .map(|p| p.glyph)
        .or_else(|| SYNTAX_GLYPHS.iter().find(|g| g.digraph == Some(d)).map(|g| g.glyph))
}

/// Human-readable table for `mimic glyphs`.
pub fn table() -> String {
    let mut s = String::from("Primitives\n  glyph  word      digraph  arity  meaning\n");
    for p in PRIMS {
        s += &format!(
            "  {:<6} {:<9} {:<8} {:<6} {}\n",
            p.glyph,
            p.word,
            p.digraph.unwrap_or(p.ascii.unwrap_or("")),
            p.arity,
            p.doc
        );
    }
    s += "\nSyntax\n  glyph  word      digraph  meaning\n";
    for g in SYNTAX_GLYPHS {
        s += &format!("  {:<6} {:<9} {:<8} {}\n", g.glyph, g.word, g.digraph.unwrap_or(""), g.doc);
    }
    s
}
