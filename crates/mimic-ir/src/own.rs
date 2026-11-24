//! Ownership (docs/design/ownership.md): each value on the heap a region
//! owns (a call's, a map's, an `if`'s, a record made or changed, a
//! parameter the function takes over, …) ends at its last use there. That
//! use takes it when it can: a call whose callee takes the argument over,
//! the base of `{ r with … }` (changed in place, then, when nothing else
//! holds it). Otherwise it is released right after, by an explicit
//! [`Op::Drop`], not at the end of the region: freed as soon as nothing
//! reads it, the next value made can take its memory.
//!
//! A field read from an owned record borrows it (no reference of its own)
//! when its uses are all here, outside any inner region, and it is not
//! what the region yields: the record then lives until the field's last
//! use, and is not handed over where the field is read.
//!
//! A last use inside a loop's regions is no last use there (the step runs
//! again): the value is released after the loop. A value whose last use is
//! inside an `if`'s branches is handed down to both: each branch ends it in
//! its own way (taken, released, yielded), or releases it first thing when
//! it does not read it. The region's result is never released: it moves out.

use crate::{Function, Inst, Op, Region, Ty, Val, owns};

/// Places the drops and moves of the whole function.
pub fn function(f: &mut Function) {
    let mut next = f.values;
    region(&mut f.body, &mut next, &[], &[]);
    f.values = next;
}

/// `given`: values an enclosing `if` handed down (owned by this region);
/// `outer`: values owned by enclosing regions that outlive this one (their
/// fields may be borrowed here too).
fn region(r: &mut Region, next: &mut u32, given: &[Val], outer: &[Val]) {
    // fields borrowed from the owned values (each with its record)
    let mut roots: Vec<Val> = given.to_vec();
    roots.extend(r.insts.iter().filter(|i| owns(i)).map(|i| i.dst));
    roots.extend_from_slice(outer);
    let in_regions = |r: &Region, v: Val| r.insts.iter().any(|i| i.op.regions().iter().any(|g| g.mentions(v)));
    let mut alias: Vec<(Val, Val)> = Vec::new();
    let fields: Vec<(usize, Val, Val)> = r.insts.iter().enumerate().filter_map(|(k, i)| if let Op::Field { base, .. } = i.op { Some((k, i.dst, base)) } else { None }).collect();
    for (k, dst, base) in fields {
        let root = alias.iter().find(|a| a.0 == base).map_or(base, |a| a.1);
        if matches!(r.insts[k].ty, Ty::List(_) | Ty::Rec(_)) && roots.contains(&root) && r.result != Some(dst) && !in_regions(r, dst) {
            if let Op::Field { borrow, .. } = &mut r.insts[k].op {
                *borrow = true;
            }
            alias.push((dst, root));
        }
    }
    // the owned values, with where each starts: handed down, then made here
    let mut owned: Vec<(usize, Val)> = given.iter().map(|v| (0, *v)).collect();
    owned.extend(r.insts.iter().enumerate().filter(|(_, i)| owns(i)).map(|(k, i)| (k + 1, i.dst)));
    // (where a drop goes, before which instruction; values handed down to an `if`)
    let mut drops: Vec<(usize, Val)> = Vec::new();
    let mut down: Vec<(usize, Val)> = Vec::new();
    for (from, v) in owned {
        if r.result == Some(v) {
            continue;
        }
        // (its fields borrowed: their uses are its uses)
        let mine: Vec<Val> = alias.iter().filter(|a| a.1 == v).map(|a| a.0).collect();
        let last = (from..r.insts.len()).rev().find(|&j| {
            let op = &r.insts[j].op;
            let ops = op.operands();
            ops.contains(&v) || mine.iter().any(|a| ops.contains(a)) || op.regions().iter().any(|g| g.mentions(v))
        });
        let Some(j) = last else {
            drops.push((from, v));
            continue;
        };
        let reads_field = r.insts[j].op.operands().iter().any(|o| mine.contains(o));
        if !reads_field && take(&mut r.insts[j].op, v) {
            continue;
        }
        if let Op::If { cond, .. } = &r.insts[j].op
            && *cond != v
        {
            down.push((j, v));
            continue;
        }
        drops.push((j + 1, v));
    }
    for (j, inst) in r.insts.iter_mut().enumerate() {
        match &mut inst.op {
            // (what lives on past the instruction: all but what it is handed)
            Op::If { then, els, .. } => {
                let mine: Vec<Val> = down.iter().filter(|d| d.0 == j).map(|d| d.1).collect();
                let live: Vec<Val> = roots.iter().copied().filter(|v| !mine.contains(v)).collect();
                region(then, next, &mine, &live);
                region(els, next, &mine, &live);
            }
            Op::Loop { done, step, .. } => {
                if let Some(d) = done {
                    region(d, next, &[], &roots);
                }
                region(step, next, &[], &roots);
            }
            _ => {}
        }
    }
    // inserted from the last place back, so earlier places stay right
    drops.sort_by_key(|&(j, v)| std::cmp::Reverse((j, v)));
    for (j, v) in drops {
        let span = r.insts.get(j.saturating_sub(1)).map(|i| i.span).unwrap_or_default();
        let dst = Val(*next);
        *next += 1;
        r.insts.insert(j, Inst { dst, ty: Ty::Unit, op: Op::Drop(v), span });
    }
}

/// Whether `op`, the last use of the owned `v`, takes it (and marks it so):
/// a call's argument its callee takes over, the base of a `with`; read
/// there once, not inside a region.
fn take(op: &mut Op, v: Val) -> bool {
    if op.regions().iter().any(|g| g.mentions(v)) {
        return false;
    }
    // (read there once)
    if op.operands().iter().filter(|o| **o == v).count() != 1 {
        return false;
    }
    match op {
        Op::Call { args, takes, moves, .. } => match args.iter().position(|a| *a == v) {
            Some(k) if takes[k] => {
                moves[k] = true;
                true
            }
            _ => false,
        },
        Op::With { base, moved, .. } | Op::Put { list: base, moved, .. } | Op::Push { list: base, moved, .. } if *base == v => {
            *moved = true;
            true
        }
        Op::Put { .. } | Op::Push { .. } => false,
        Op::With { fields, moves, .. } => match fields.iter().position(|f| f.1 == v) {
            Some(k) => {
                moves[k] = true;
                true
            }
            // (a number a change reads)
            None => false,
        },
        Op::Record { fields, moves, .. } => {
            let k = fields.iter().position(|f| *f == v).expect("an operand");
            moves[k] = true;
            true
        }
        // a record an embedded expression reads last: the old path moves it
        // on where it can (`start ▷ evolve(…)` changes it in place), else
        // releases it after the expression
        // (a list too, of any element type but text: `push(acc, x)` at the
        // list's last use grows it in place. Text stays lent: moving it into
        // the old path made `roman`, `semver` and `url` slower)
        Op::Embed { uses, moves, .. } => match uses.iter().position(|u| u.1 == v) {
            Some(k) if matches!(uses[k].2, Ty::Rec(_)) || matches!(uses[k].2, Ty::List(t) if !matches!(t, mimic_check::types::Ty::Arr(mimic_check::types::El::Char, _))) => {
                moves[k] = true;
                true
            }
            _ => false,
        },
        _ => false,
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
        super::region(&mut body, &mut next, &[], &[]);
        assert!(drops(&body).is_empty(), "the result is not released");
        // the same list not yielded: released after the loop, not in it
        body.result = Some(Val(2));
        super::region(&mut body, &mut next, &[], &[]);
        assert_eq!(drops(&body), vec![(3, Val(1))]);
        let Op::Loop { step, .. } = &body.insts[2].op else { panic!() };
        assert!(drops(step).is_empty());
    }

    fn call(dst: u32, ty: Ty, args: Vec<Val>, takes: Vec<bool>) -> Inst {
        let moves = vec![false; args.len()];
        inst(dst, ty, Op::Call { site: 0, callee: 0, args, takes, moves })
    }

    #[test]
    fn a_callee_that_takes_its_argument_gets_it_at_the_last_use() {
        // %1 = map ⍳%0; %2 = call f(%1) (f takes it); %3 = call g(%1) (takes it); result %3
        let mut body = Region {
            insts: vec![inst(0, Ty::I64, Op::Param(0)), inst(1, list(), Op::Map(Tree::Iota(Val(0)))), call(2, Ty::I64, vec![Val(1)], vec![true]), call(3, list(), vec![Val(1)], vec![true])],
            result: Some(Val(3)),
        };
        let mut next = 4;
        super::region(&mut body, &mut next, &[], &[]);
        let moved = |k: usize| if let Op::Call { moves, .. } = &body.insts[k].op { moves[0] } else { panic!() };
        // the first is not the last use (lent); the second takes it, so no drop
        assert!(!moved(2) && moved(3));
        assert!(drops(&body).is_empty());
    }

    #[test]
    fn an_if_hands_its_branches_the_value_they_end() {
        // %1 = map ⍳%0; %4 = if %0 { %2 = call f(%1) (takes it); yield %2 } else { %3 = 1; yield %3 }
        let then = Region { insts: vec![call(2, Ty::I64, vec![Val(1)], vec![true])], result: Some(Val(2)) };
        let els = Region { insts: vec![inst(3, Ty::I64, Op::Int(1))], result: Some(Val(3)) };
        let mut body = Region {
            insts: vec![inst(0, Ty::Bool, Op::Param(0)), inst(1, list(), Op::Map(Tree::Iota(Val(0)))), inst(4, Ty::I64, Op::If { cond: Val(0), then, els })],
            result: Some(Val(4)),
        };
        let mut next = 5;
        super::region(&mut body, &mut next, &[], &[]);
        assert!(drops(&body).is_empty(), "not released after the if");
        let Op::If { then, els, .. } = &body.insts[2].op else { panic!() };
        assert!(matches!(&then.insts[0].op, Op::Call { moves, .. } if moves[0]), "taken in one branch");
        assert_eq!(drops(els), vec![(0, Val(1))], "released first thing in the other");
    }

    #[test]
    fn a_field_borrows_its_record_which_lives_until_the_field_is_done() {
        // %1 = take; %2 = %1.field 0 (a list); %3 = len %2; %4 = call f(%1) (takes it); %5 = len %2; result %5
        let rec = Ty::Rec(C::Obj(0));
        let mut body = Region {
            insts: vec![
                inst(1, rec, Op::Take(0)),
                inst(2, list(), Op::Field { base: Val(1), tid: 0, k: 0, borrow: false }),
                inst(3, Ty::I64, Op::Len(Val(2))),
                call(4, Ty::I64, vec![Val(1)], vec![true]),
                inst(5, Ty::I64, Op::Len(Val(2))),
            ],
            result: Some(Val(5)),
        };
        let mut next = 6;
        super::region(&mut body, &mut next, &[], &[]);
        assert!(matches!(body.insts[1].op, Op::Field { borrow: true, .. }));
        // the call comes before the field's last use: lent, and the record
        // released after that use
        assert!(matches!(&body.insts[3].op, Op::Call { moves, .. } if !moves[0]));
        assert_eq!(drops(&body), vec![(5, Val(1))]);
    }
}
