//! Sinking: an instruction that cannot fail and has no effect, whose value
//! only one branch of a later `if` uses, moves into that branch, so it is
//! computed only when the branch runs. The old code generator gets this by
//! compiling a `let` where it is used; the IR defines every `let` where it
//! stands, and LLVM does not always move the work back (it may turn the
//! branch into selects instead and compute both sides: pip's edge test).
//!
//! Only into the branches of an `if`, never into a loop's regions (there
//! the work would run once per step instead of once).

use crate::{Function, Op, Region, Val};
use mimic_syntax::ast::BinOp;

/// Sinks what can be sunk in the whole function.
pub fn function(f: &mut Function) {
    region(&mut f.body);
}

fn region(r: &mut Region) {
    // from the last instruction back: what a sunk instruction reads is
    // then considered after it, and lands in front of it
    let mut i = r.insts.len();
    while i > 0 {
        i -= 1;
        if !movable(&r.insts[i].op) {
            continue;
        }
        let v = r.insts[i].dst;
        if r.result == Some(v) {
            continue;
        }
        if let Some((j, arm)) = sole_branch(r, i + 1, v) {
            let inst = r.insts.remove(i);
            // the `if` was at `j`, after `i`: one place earlier now
            let Op::If { then, els, .. } = &mut r.insts[j - 1].op else { unreachable!() };
            let target = if arm { then } else { els };
            target.insts.insert(0, inst);
        }
    }
    for inst in &mut r.insts {
        match &mut inst.op {
            Op::If { then, els, .. } => {
                region(then);
                region(els);
            }
            Op::Loop { done, step, .. } => {
                if let Some(d) = done {
                    region(d);
                }
                region(step);
            }
            _ => {}
        }
    }
}

/// Whether the instruction can be computed anywhere it is visible, or not
/// at all, without changing what the program does.
fn movable(op: &Op) -> bool {
    match op {
        Op::Int(_) | Op::Float(_) | Op::Bool(_) | Op::Unit | Op::Un(..) | Op::Make(_) | Op::Get(..) | Op::Set(..) | Op::Len(_) => true,
        // a division by zero stops the program: it must stay where it is
        Op::Bin(op, ..) => !matches!(op, BinOp::Div | BinOp::Mod | BinOp::IntDiv),
        // (a reduction is work: it stays where the program does it)
        // (a record made, or a field taken, is owned: it stays)
        Op::Param(_) | Op::Take(_) | Op::Call { .. } | Op::If { .. } | Op::Index(..) | Op::Reduce(..) | Op::Map(_) | Op::Embed { .. } | Op::Loop { .. } | Op::Drop(_) | Op::Record { .. } | Op::Field { .. } | Op::With { .. } | Op::Put { .. } | Op::Push { .. } => false,
    }
}

/// The one `if` among `r.insts[from..]` (its index) whose one branch
/// (true: `then`) holds every use of `v`, when no other instruction there
/// reads it.
fn sole_branch(r: &Region, from: usize, v: Val) -> Option<(usize, bool)> {
    let mut found = None;
    for (k, inst) in r.insts.iter().enumerate().skip(from) {
        if reads(&inst.op, v) {
            return None;
        }
        let arms: Vec<(bool, &Region)> = match &inst.op {
            Op::If { then, els, .. } => vec![(true, then), (false, els)],
            Op::Loop { done, step, .. } => {
                if done.as_ref().is_some_and(|d| mentions(d, v)) || mentions(step, v) {
                    return None;
                }
                continue;
            }
            _ => continue,
        };
        for (arm, reg) in arms {
            if mentions(reg, v) {
                if found.is_some() {
                    return None;
                }
                found = Some((k, arm));
            }
        }
    }
    found
}

/// Whether the operation reads `v` itself (not inside its regions).
fn reads(op: &Op, v: Val) -> bool {
    op.operands().contains(&v)
}

/// Whether anything in `r` (nested regions included) reads `v`.
fn mentions(r: &Region, v: Val) -> bool {
    r.result == Some(v)
        || r.insts.iter().any(|i| {
            reads(&i.op, v)
                || match &i.op {
                    Op::If { then, els, .. } => mentions(then, v) || mentions(els, v),
                    Op::Loop { done, step, .. } => done.as_ref().is_some_and(|d| mentions(d, v)) || mentions(step, v),
                    _ => false,
                }
        })
}

#[cfg(test)]
mod tests {
    use crate::{Inst, Op, Region, Ty, Val};
    use mimic_syntax::ast::BinOp;

    fn inst(dst: u32, ty: Ty, op: Op) -> Inst {
        Inst { dst: Val(dst), ty, op, span: Default::default() }
    }

    #[test]
    fn sinks_into_the_one_branch_that_uses_it() {
        // %1 = %0 * %0; %2 = %0 > %0; if %2 { yield %1 } else { yield %0 }
        let mut r = Region {
            insts: vec![
                inst(1, Ty::I64, Op::Bin(BinOp::Mul, Val(0), Val(0))),
                inst(2, Ty::Bool, Op::Bin(BinOp::Gt, Val(0), Val(0))),
                inst(
                    3,
                    Ty::I64,
                    Op::If {
                        cond: Val(2),
                        then: Region { insts: vec![], result: Some(Val(1)) },
                        els: Region { insts: vec![], result: Some(Val(0)) },
                    },
                ),
            ],
            result: Some(Val(3)),
        };
        super::region(&mut r);
        assert_eq!(r.insts.len(), 2);
        let Op::If { then, .. } = &r.insts[1].op else { panic!("the if moved") };
        assert_eq!(then.insts.len(), 1);
        assert_eq!(then.insts[0].dst, Val(1));
    }

    #[test]
    fn a_division_stays() {
        let mut r = Region {
            insts: vec![
                inst(1, Ty::I64, Op::Bin(BinOp::IntDiv, Val(0), Val(0))),
                inst(
                    2,
                    Ty::I64,
                    Op::If {
                        cond: Val(0),
                        then: Region { insts: vec![], result: Some(Val(1)) },
                        els: Region { insts: vec![], result: Some(Val(0)) },
                    },
                ),
            ],
            result: Some(Val(2)),
        };
        super::region(&mut r);
        assert_eq!(r.insts.len(), 2);
    }
}
