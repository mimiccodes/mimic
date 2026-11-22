//! Ownership (docs/design/ownership.md), first part: each list a region owns
//! (a call's, a map's, an `if`'s, an embedded expression's) is released
//! right after its last use there, by an explicit [`Op::Drop`], instead of
//! at the end of the region: a list is freed as soon as nothing reads it
//! any more, and the next one made can take its memory.
//!
//! A last use inside a loop's regions is no last use there (the step runs
//! again): the list is released after the loop. One read inside a branch
//! counts as a use of the `if` (released after it). The region's result is
//! never released: it moves out.

use crate::{Function, Inst, Op, Region, Ty, Val, owns};

/// Places the drops of the whole function.
pub fn function(f: &mut Function) {
    let mut next = f.values;
    region(&mut f.body, &mut next);
    f.values = next;
}

fn region(r: &mut Region, next: &mut u32) {
    for inst in &mut r.insts {
        match &mut inst.op {
            Op::If { then, els, .. } => {
                region(then, next);
                region(els, next);
            }
            Op::Loop { done, step, .. } => {
                if let Some(d) = done {
                    region(d, next);
                }
                region(step, next);
            }
            _ => {}
        }
    }
    // (after which instruction each owned list is read for the last time)
    let mut drops: Vec<(usize, Val)> = Vec::new();
    for (k, i) in r.insts.iter().enumerate() {
        if !owns(i) || r.result == Some(i.dst) {
            continue;
        }
        let v = i.dst;
        let last = (k + 1..r.insts.len()).rev().find(|&j| {
            let op = &r.insts[j].op;
            op.operands().contains(&v) || op.regions().iter().any(|g| g.mentions(v))
        });
        drops.push((last.unwrap_or(k), v));
    }
    // inserted from the last place back, so earlier places stay right
    drops.sort_by_key(|&(j, v)| std::cmp::Reverse((j, v)));
    for (j, v) in drops {
        let span = r.insts[j].span;
        let dst = Val(*next);
        *next += 1;
        r.insts.insert(j + 1, Inst { dst, ty: Ty::Unit, op: Op::Drop(v), span });
    }
}

#[cfg(test)]
mod tests {
    use crate::{Function, Inst, Op, Region, Tree, Ty, Val};
    use mimic_check::types::{El, Ty as C};
    use mimic_syntax::glyphs::Prim;

    fn inst(dst: u32, ty: Ty, op: Op) -> Inst {
        Inst { dst: Val(dst), ty, op, span: Default::default() }
    }

    fn list() -> Ty {
        Ty::List(C::Arr(El::I64, 0))
    }

    fn drops(r: &Region) -> Vec<(usize, Val)> {
        r.insts.iter().enumerate().filter_map(|(k, i)| if let Op::Drop(v) = i.op { Some((k, v)) } else { None }).collect()
    }

    #[test]
    fn released_after_its_last_use_not_at_the_end() {
        // %1 = map ⍳%0; %2 = reduce Sum %1[i]; %3 = %0 * %0 (no list); result %3
        let mut f = Function {
            name: "t".into(),
            params: vec![("n".into(), Ty::I64)],
            ret: Ty::I64,
            body: Region {
                insts: vec![
                    inst(0, Ty::I64, Op::Param(0)),
                    inst(1, list(), Op::Map(Tree::Iota(Val(0)))),
                    inst(2, Ty::I64, Op::Reduce(Prim::Sum, Tree::Leaf(Val(1), El::I64), El::I64)),
                    inst(3, Ty::I64, Op::Bin(mimic_syntax::ast::BinOp::Mul, Val(0), Val(0))),
                ],
                result: Some(Val(3)),
            },
            values: 4,
        };
        super::function(&mut f);
        assert_eq!(drops(&f.body), vec![(3, Val(1))]);
        assert_eq!(f.values, 5);
    }

    #[test]
    fn a_result_moves_out_and_a_loop_holds_its_list() {
        // %1 = map ⍳%0; loop %3 = %0 { step: reduce Sum %1[i] }; result %1
        let step = Region { insts: vec![inst(4, Ty::I64, Op::Reduce(Prim::Sum, Tree::Leaf(Val(1), El::I64), El::I64))], result: Some(Val(4)) };
        let mut body = Region {
            insts: vec![
                inst(0, Ty::I64, Op::Param(0)),
                inst(1, list(), Op::Map(Tree::Iota(Val(0)))),
                inst(2, Ty::I64, Op::Loop { init: Val(0), var: Val(3), count: Some(Val(0)), done: None, step }),
            ],
            result: Some(Val(1)),
        };
        let mut next = 5;
        super::region(&mut body, &mut next);
        assert!(drops(&body).is_empty(), "the result is not released");
        // the same list not yielded: released after the loop, not in it
        body.result = Some(Val(2));
        super::region(&mut body, &mut next);
        assert_eq!(drops(&body), vec![(3, Val(1))]);
        let Op::Loop { step, .. } = &body.insts[2].op else { panic!() };
        assert!(drops(step).is_empty());
    }
}
