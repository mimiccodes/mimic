//! Mimic IR: the intermediate representation between the checked syntax tree
//! and LLVM (docs/design/ir.md).
//!
//! A function is a tree of regions: each region is a list of instructions in
//! SSA form (every value is defined once) ending in the value it yields.
//! Control flow is structured: an `if` is one instruction holding a region
//! for each branch, not jumps between blocks, which keeps loops easy to find
//! and lets nets and GPU kernels be made from the same IR later.
//!
//! The compiler moves onto it one function at a time: [`lower::function`]
//! lowers what it covers and returns `None` for anything else, which then
//! takes today's code generator. This first step covers functions of plain
//! numbers and truth values: literals, parameters, `let`, arithmetic and
//! comparisons, `and`/`or`, `if` and calls.

pub mod lower;

use mimic_syntax::ast::{BinOp, UnOp};
use std::fmt::{self, Write as _};

/// The type of an IR value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Unit,
    I64,
    F64,
    Bool,
    /// A value kept in registers: a list of 1 … 16 numbers or truth values,
    /// or a tuple or record of numbers, truth values and such lists (at
    /// most 64 numbers). The checker's type says which.
    Agg(mimic_check::types::Ty),
}

impl Ty {
    pub fn name(self) -> String {
        match self {
            Ty::Unit => "unit".into(),
            Ty::I64 => "i64".into(),
            Ty::F64 => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Agg(t) => format!("regs {t}"),
        }
    }

    /// The IR type of a checker type, when the IR covers it (the program,
    /// for a record's fields).
    pub fn of(p: &mimic_check::Program, t: mimic_check::types::Ty) -> Option<Ty> {
        use mimic_check::types::Ty as C;
        Some(match t {
            C::Unit => Ty::Unit,
            C::I64 => Ty::I64,
            C::F64 => Ty::F64,
            C::Bool => Ty::Bool,
            _ if parts(p, t).is_some() => Ty::Agg(t),
            _ => return None,
        })
    }
}

/// A short list's element type and length (1 … 16 numbers or truth values).
pub fn short_list(t: mimic_check::types::Ty) -> Option<(mimic_check::types::El, u32)> {
    use mimic_check::types::{El, Ty as C};
    match t {
        C::Arr(el @ (El::I64 | El::F64 | El::Bool), n) if n > 0 => Some((el, n as u32)),
        _ => None,
    }
}

/// The parts of a value kept in registers, in order: a short list's
/// elements, a tuple's, a record's fields as declared. The same rule as the
/// code generator's: numbers, truth values and short lists, 64 numbers at
/// most. None for anything else.
pub fn parts(p: &mimic_check::Program, t: mimic_check::types::Ty) -> Option<Vec<mimic_check::types::Ty>> {
    use mimic_check::types::{self as ct, Ty as C};
    if let Some((el, n)) = short_list(t) {
        return Some(vec![C::of_el(el); n as usize]);
    }
    let ps: Vec<C> = match t {
        C::Tup(id) => ct::tuple_elems(id),
        C::Obj(id) => {
            let tid = ct::obj_tid(id);
            let mimic_syntax::ast::TypeKind::Record(fs) = &p.types[tid].decl.kind else { return None };
            fs.iter().map(|(_, ft, _)| ct::field_type(p, tid, t, ft)).collect()
        }
        _ => return None,
    };
    let mut n = 0usize;
    for x in &ps {
        match x {
            C::I64 | C::F64 | C::Bool => n += 1,
            _ => n += short_list(*x)?.1 as usize,
        }
    }
    (!ps.is_empty() && n <= 64).then_some(ps)
}

/// A value: the result of one instruction (or a function parameter).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Val(pub u32);

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Int(i64),
    Float(f64),
    Bool(bool),
    Unit,
    /// The function's parameter number `n`.
    Param(u32),
    /// An operator on two values, both always evaluated (`and`/`or` become
    /// an [`Op::If`]).
    Bin(BinOp, Val, Val),
    Un(UnOp, Val),
    /// A call of a user function. `site` is the call's syntax node (what
    /// the code generator keys the callee's specialisation and placement by).
    Call { site: usize, callee: mimic_check::FnId, args: Vec<Val> },
    /// `if cond { then } else { els }`: the value the branch taken yields.
    If { cond: Val, then: Region, els: Region },
    /// A value kept in registers made of its parts (a tuple's elements, a
    /// short list's, a record's fields in declaration order).
    Make(Vec<Val>),
    /// Part `k` of a value kept in registers.
    Get(Val, u32),
    /// The value with part `k` replaced (`{ r with f: x }`).
    Set(Val, u32, Val),
}

#[derive(Clone, Debug)]
pub struct Inst {
    pub dst: Val,
    pub ty: Ty,
    pub op: Op,
    /// Where it comes from in the source (for debug information).
    pub span: mimic_syntax::Span,
}

/// A list of instructions and the value it yields.
#[derive(Clone, Debug, Default)]
pub struct Region {
    pub insts: Vec<Inst>,
    pub result: Option<Val>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
    pub body: Region,
    /// How many values the function defines (parameters included).
    pub values: u32,
}

// ---- printing ---------------------------------------------------------------

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ps: Vec<String> = self.params.iter().enumerate().map(|(i, (n, t))| format!("%{i} {n}: {}", t.name())).collect();
        writeln!(f, "fn {}({}) -> {} {{", self.name, ps.join(", "), self.ret.name())?;
        let mut s = String::new();
        region(&mut s, &self.body, 1);
        f.write_str(&s)?;
        writeln!(f, "}}")
    }
}

fn region(out: &mut String, r: &Region, depth: usize) {
    let pad = "    ".repeat(depth);
    for i in &r.insts {
        let _ = write!(out, "{pad}{} = ", i.dst);
        match &i.op {
            Op::Int(n) => {
                let _ = writeln!(out, "{n}: i64");
            }
            Op::Float(x) => {
                let _ = writeln!(out, "{x:?}: f64");
            }
            Op::Bool(b) => {
                let _ = writeln!(out, "{b}");
            }
            Op::Unit => {
                let _ = writeln!(out, "()");
            }
            Op::Param(n) => {
                let _ = writeln!(out, "param {n}");
            }
            Op::Bin(op, a, b) => {
                let _ = writeln!(out, "{a} {} {b}: {}", op.symbol(), i.ty.name());
            }
            Op::Un(op, a) => {
                let _ = writeln!(out, "{op:?} {a}: {}", i.ty.name());
            }
            Op::Call { callee, args, .. } => {
                let a: Vec<String> = args.iter().map(|v| v.to_string()).collect();
                let _ = writeln!(out, "call fn#{callee}({}): {}", a.join(", "), i.ty.name());
            }
            Op::Make(xs) => {
                let a: Vec<String> = xs.iter().map(|v| v.to_string()).collect();
                let _ = writeln!(out, "make({}): {}", a.join(", "), i.ty.name());
            }
            Op::Get(v, k) => {
                let _ = writeln!(out, "{v}.{k}: {}", i.ty.name());
            }
            Op::Set(v, k, x) => {
                let _ = writeln!(out, "{v} with .{k} = {x}: {}", i.ty.name());
            }
            Op::If { cond, then, els } => {
                let _ = writeln!(out, "if {cond}: {} {{", i.ty.name());
                region(out, then, depth + 1);
                let _ = writeln!(out, "{pad}}} else {{");
                region(out, els, depth + 1);
                let _ = writeln!(out, "{pad}}}");
            }
        }
    }
    if let Some(v) = r.result {
        let _ = writeln!(out, "{pad}yield {v}");
    }
}

// ---- checking ----------------------------------------------------------------

/// Checks that every value is defined once, before it is used, in a region
/// that encloses the use, and that operands have the types the operations
/// need. Run after lowering (and later after every pass).
pub fn verify(f: &Function) -> Result<(), String> {
    let mut types: Vec<Option<Ty>> = vec![None; f.values as usize];
    let mut scope: Vec<Val> = Vec::new();
    verify_region(f, &f.body, &mut types, &mut scope)?;
    if let Some(v) = f.body.result {
        let t = types[v.0 as usize].ok_or_else(|| format!("{}: the result {v} is not defined", f.name))?;
        if t != f.ret {
            return Err(format!("{}: yields {} for a {} result", f.name, t.name(), f.ret.name()));
        }
    }
    Ok(())
}

fn verify_region(f: &Function, r: &Region, types: &mut Vec<Option<Ty>>, scope: &mut Vec<Val>) -> Result<(), String> {
    let mark = scope.len();
    let seen = |scope: &Vec<Val>, v: Val| scope.contains(&v);
    for i in &r.insts {
        let ty = |v: Val, types: &Vec<Option<Ty>>| -> Result<Ty, String> {
            if !seen(scope, v) {
                return Err(format!("{}: {v} is used where it is not defined", f.name));
            }
            types[v.0 as usize].ok_or_else(|| format!("{}: {v} has no type", f.name))
        };
        match &i.op {
            Op::Int(_) | Op::Float(_) | Op::Bool(_) | Op::Unit => {}
            Op::Param(n) => {
                let want = f.params.get(*n as usize).map(|p| p.1).ok_or_else(|| format!("{}: no parameter {n}", f.name))?;
                if want != i.ty {
                    return Err(format!("{}: parameter {n} is {}, not {}", f.name, want.name(), i.ty.name()));
                }
            }
            Op::Bin(_, a, b) => {
                ty(*a, types)?;
                ty(*b, types)?;
            }
            Op::Un(_, a) => {
                ty(*a, types)?;
            }
            Op::Call { args, .. } | Op::Make(args) => {
                for a in args {
                    ty(*a, types)?;
                }
            }
            Op::Get(v, _) => {
                if !matches!(ty(*v, types)?, Ty::Agg(_)) {
                    return Err(format!("{}: {v} has no parts", f.name));
                }
            }
            Op::Set(v, _, x) => {
                if !matches!(ty(*v, types)?, Ty::Agg(_)) {
                    return Err(format!("{}: {v} has no parts", f.name));
                }
                ty(*x, types)?;
            }
            Op::If { cond, then, els } => {
                if ty(*cond, types)? != Ty::Bool {
                    return Err(format!("{}: the condition {cond} is not a truth value", f.name));
                }
                for b in [then, els] {
                    verify_region(f, b, types, scope)?;
                    if i.ty != Ty::Unit {
                        let v = b.result.ok_or_else(|| format!("{}: a branch of {} yields nothing", f.name, i.dst))?;
                        if types[v.0 as usize] != Some(i.ty) {
                            return Err(format!("{}: a branch of {} yields another type", f.name, i.dst));
                        }
                    }
                }
            }
        }
        let d = i.dst.0 as usize;
        if d >= types.len() {
            return Err(format!("{}: {} is past the function's values", f.name, i.dst));
        }
        if types[d].is_some() {
            return Err(format!("{}: {} is defined twice", f.name, i.dst));
        }
        types[d] = Some(i.ty);
        scope.push(i.dst);
    }
    if let Some(v) = r.result
        && !seen(scope, v)
    {
        return Err(format!("{}: yields {v}, which is not defined here", f.name));
    }
    scope.truncate(mark);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_syntax::Span;

    fn inst(dst: u32, ty: Ty, op: Op) -> Inst {
        Inst { dst: Val(dst), ty, op, span: Span::default() }
    }

    fn func(insts: Vec<Inst>, result: u32, values: u32) -> Function {
        Function { name: "f".into(), params: vec![("a".into(), Ty::I64)], ret: Ty::I64, body: Region { insts, result: Some(Val(result)) }, values }
    }

    #[test]
    fn verify_accepts_a_good_function() {
        let f = func(vec![inst(0, Ty::I64, Op::Param(0)), inst(1, Ty::I64, Op::Int(1)), inst(2, Ty::I64, Op::Bin(BinOp::Add, Val(0), Val(1)))], 2, 3);
        assert_eq!(verify(&f), Ok(()));
        assert!(f.to_string().contains("%2 = %0 + %1: i64"), "{f}");
    }

    #[test]
    fn verify_rejects_a_value_defined_twice() {
        let f = func(vec![inst(0, Ty::I64, Op::Param(0)), inst(0, Ty::I64, Op::Int(1))], 0, 1);
        assert!(verify(&f).unwrap_err().contains("defined twice"));
    }

    #[test]
    fn verify_rejects_a_use_before_its_definition() {
        let f = func(vec![inst(0, Ty::I64, Op::Bin(BinOp::Add, Val(1), Val(1))), inst(1, Ty::I64, Op::Int(1))], 0, 2);
        assert!(verify(&f).unwrap_err().contains("not defined"));
    }

    #[test]
    fn verify_rejects_a_value_from_inside_a_branch() {
        // %2 is defined in the `then` region: it does not exist after the `if`
        let then = Region { insts: vec![inst(2, Ty::I64, Op::Int(5))], result: Some(Val(2)) };
        let els = Region { insts: vec![inst(3, Ty::I64, Op::Int(6))], result: Some(Val(3)) };
        let f = func(
            vec![inst(0, Ty::I64, Op::Param(0)), inst(1, Ty::Bool, Op::Bool(true)), inst(4, Ty::I64, Op::If { cond: Val(1), then, els }), inst(5, Ty::I64, Op::Bin(BinOp::Add, Val(2), Val(4)))],
            5,
            6,
        );
        assert!(verify(&f).unwrap_err().contains("%2"));
    }

    #[test]
    fn verify_rejects_a_condition_that_is_not_a_truth_value() {
        let then = Region { insts: vec![], result: Some(Val(0)) };
        let els = Region { insts: vec![], result: Some(Val(0)) };
        let f = func(vec![inst(0, Ty::I64, Op::Param(0)), inst(1, Ty::I64, Op::If { cond: Val(0), then, els })], 1, 2);
        assert!(verify(&f).unwrap_err().contains("truth value"));
    }
}
