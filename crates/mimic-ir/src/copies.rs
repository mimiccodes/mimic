//! `mimic check --copies`: the copies a program still makes, and why.
//!
//! After [`crate::own`] has placed the moves, a change that is not handed
//! its value (`put`, `push`, `{ … with … }` not `moved`) works on a value
//! something else still holds, so it copies it every time it runs; and a
//! value lent to a function that takes its argument over is copied there
//! if that function changes it. Functions the IR does not cover are listed
//! with the reason, as their copies are not analysed.

use crate::{Function, Op, Region, Val};
use mimic_check::{FnId, Program};
use std::collections::BTreeSet;
use std::sync::Mutex;

/// One line of the report.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Note {
    /// The file as shown to users, then the place in it.
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub text: String,
}

#[derive(Default)]
struct Report {
    copies: BTreeSet<Note>,
    /// (function, its note): functions the IR does not cover
    missed: Vec<(String, Note)>,
    /// functions with a version the IR covers
    covered: BTreeSet<String>,
}

static REPORT: Mutex<Option<Report>> = Mutex::new(None);

/// Starts collecting (before the program is compiled).
pub fn start() {
    *REPORT.lock().unwrap() = Some(Report::default());
}

pub(crate) fn on() -> bool {
    REPORT.lock().unwrap().is_some()
}

/// The report, sorted by place; a function is listed as left to the old
/// path only when none of its versions goes through the IR.
pub fn finish() -> Vec<Note> {
    let Some(r) = REPORT.lock().unwrap().take() else { return Vec::new() };
    let mut out: BTreeSet<Note> = r.copies;
    for (f, n) in r.missed {
        if !r.covered.contains(&f) {
            out.insert(n);
        }
    }
    out.into_iter().collect()
}

/// The user's own code (not the standard library's).
fn mine(p: &Program, fid: FnId) -> bool {
    p.files[p.fns[fid].file].package == p.root_package
}

fn note(p: &Program, fid: FnId, span: mimic_syntax::Span, text: String) -> Note {
    let file = &p.files[p.fns[fid].file];
    Note { file: file.display.clone(), line: span.line, col: span.col, text }
}

/// The source of `span`, on one line and cut short.
fn source(p: &Program, fid: FnId, span: mimic_syntax::Span) -> String {
    let src = &p.files[p.fns[fid].file].src;
    let s = src.get(span.start..span.end).unwrap_or("");
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 60 { format!("{}…", s.chars().take(59).collect::<String>()) } else { s }
}

/// A function the IR does not cover.
pub(crate) fn missed(p: &Program, fid: FnId, why: &str) {
    if !mine(p, fid) {
        return;
    }
    let name = p.fn_path(fid).join("::");
    let short = p.fns[fid].name.clone();
    let n = note(p, fid, p.fns[fid].span, format!("`{short}` is compiled by the old code generator ({why}); its copies are not listed"));
    if let Some(r) = REPORT.lock().unwrap().as_mut() {
        r.missed.push((name, n));
    }
}

/// The copies left in a function the IR covers.
pub(crate) fn scan(p: &Program, fid: FnId, f: &Function) {
    if !mine(p, fid) {
        return;
    }
    let mut found = Vec::new();
    walk(p, fid, f, &f.body, None, &mut found);
    if let Some(r) = REPORT.lock().unwrap().as_mut() {
        r.covered.insert(f.name.clone());
        r.copies.extend(found);
    }
}

/// `step`: the values a loop's step region defines (any deeper), when
/// inside one: what it reads from outside is still held for the next step.
fn walk(p: &Program, fid: FnId, f: &Function, r: &Region, step: Option<&BTreeSet<Val>>, out: &mut Vec<Note>) {
    for (j, i) in r.insts.iter().enumerate() {
        let lent = |v: Val| why(f, r, j, v, step);
        let at = |what: String| note(p, fid, i.span, what);
        match &i.op {
            Op::Put { list, moved: false, .. } => out.push(at(format!("`{}` copies the list: {}", source(p, fid, i.span), lent(*list)))),
            Op::Push { list, moved: false, .. } => out.push(at(format!("`{}` copies the list: {}", source(p, fid, i.span), lent(*list)))),
            Op::With { base, moved: false, .. } => out.push(at(format!("`{}` copies the record: {}", source(p, fid, i.span), lent(*base)))),
            Op::Call { args, takes, moves, callee, .. } => {
                for (k, a) in args.iter().enumerate() {
                    if takes[k] && !moves[k] {
                        let name = &p.fns[*callee].name;
                        out.push(at(format!("`{name}` changes argument {} in place, so it copies it here: {}", k + 1, lent(*a))));
                    }
                }
            }
            _ => {}
        }
        if let Op::Loop { done, step: body, .. } = &i.op {
            let mut inside = BTreeSet::new();
            defs(body, &mut inside);
            if let Some(d) = done {
                defs(d, &mut inside);
            }
            if let Op::Loop { var, .. } = &i.op {
                inside.insert(*var);
            }
            if let Some(d) = done {
                walk(p, fid, f, d, Some(&inside), out);
            }
            walk(p, fid, f, body, Some(&inside), out);
        } else {
            for g in i.op.regions() {
                walk(p, fid, f, g, step, out);
            }
        }
    }
}

fn defs(r: &Region, out: &mut BTreeSet<Val>) {
    for i in &r.insts {
        out.insert(i.dst);
        for g in i.op.regions() {
            defs(g, out);
        }
    }
}

/// Why `v`, read by instruction `j` of `r`, is not handed over there.
fn why(f: &Function, r: &Region, j: usize, v: Val, step: Option<&BTreeSet<Val>>) -> String {
    if step.is_some_and(|s| !s.contains(&v)) {
        return "it comes from outside the loop, and every step reads it again".into();
    }
    // a later read of it, anywhere after this instruction (the function's
    // instructions in the order they are written)
    let mut order = Vec::new();
    flat(&f.body, &mut order);
    let here = &r.insts[j] as *const crate::Inst;
    let later = order.iter().skip_while(|i| !std::ptr::eq(**i, here)).skip(1).find(|i| !matches!(i.op, Op::Drop(_)) && i.op.operands().contains(&v));
    let name = match def(&f.body, v).map(|i| &i.op) {
        Some(Op::Param(n) | Op::Take(n)) => format!("`{}`", f.params[*n as usize].0),
        _ => "it".into(),
    };
    if let Some(i) = later {
        return format!("{name} is used again after this (line {})", i.span.line);
    }
    match def(&f.body, v).map(|i| &i.op) {
        Some(Op::Param(_)) => format!("{name} is a parameter this function only borrows: its callers keep it"),
        Some(Op::Field { borrow: true, .. }) => "it is a field of a record that is still in use".into(),
        _ if r.result == Some(v) => "it is also the result here".into(),
        _ => "something else still holds it".into(),
    }
}

/// Every instruction, in the order written (a region's before what follows it).
fn flat<'a>(r: &'a Region, out: &mut Vec<&'a crate::Inst>) {
    for i in &r.insts {
        for g in i.op.regions() {
            flat(g, out);
        }
        out.push(i);
    }
}

fn def(r: &Region, v: Val) -> Option<&crate::Inst> {
    for i in &r.insts {
        if i.dst == v {
            return Some(i);
        }
        for g in i.op.regions() {
            if let Some(d) = def(g, v) {
                return Some(d);
            }
        }
    }
    None
}
