//! A list field changed under a `let` and stored back, as one change of the
//! record: `let t = put(s.table, h, x)` … `{ s with table: t }` becomes
//! `{ s with table: put [h] = x }`, which takes the field out of the record
//! (when the record is held once) and changes it in place, where the `put`
//! alone works on a field the record still holds, and so on a copy.
//!
//! Only when the changed list is used for nothing else and the field it
//! starts from for nothing but that change. The change then happens where
//! the `with` is: what reads the field in between still sees the old list,
//! as it would. The `with` must not be inside a loop the change is outside
//! of (the change would run again with every step).

use crate::{Change, Function, Inst, Op, Region, Val};
use std::collections::HashMap;

pub fn function(f: &mut Function) {
    let mut uses: HashMap<Val, usize> = HashMap::new();
    count(&f.body, &mut uses);
    let mut defs: HashMap<Val, Op> = HashMap::new();
    collect(&f.body, &mut defs);
    // (the last value of a chain → (record, field, changes innermost first, the chain's values))
    let mut plans: HashMap<Val, (Val, u32, Vec<Change>, Vec<Val>)> = HashMap::new();
    for (&t, op) in &defs {
        if !matches!(op, Op::Put { .. } | Op::Push { .. }) {
            continue;
        }
        let mut chain = Vec::new();
        let mut vals = vec![t];
        let mut cur = t;
        let start = loop {
            match defs.get(&cur) {
                Some(Op::Put { list, index, value, .. }) => {
                    chain.push(Change::Put(*index, *value));
                    cur = *list;
                }
                Some(Op::Push { list, value, .. }) => {
                    chain.push(Change::Push(*value));
                    cur = *list;
                }
                Some(Op::Field { base, k, .. }) => {
                    // (the field read, gone too when the chain is its one use)
                    if cur != t && uses.get(&cur) == Some(&1) {
                        vals.push(cur);
                    }
                    break Some((*base, *k));
                }
                _ => break None,
            }
            // (each link used once: by the next change)
            if uses.get(&cur) != Some(&1) {
                break None;
            }
            vals.push(cur);
        };
        let Some((base, k)) = start else { continue };
        chain.reverse();
        plans.insert(t, (base, k, chain, vals));
    }
    // keep a plan only if every use of its value is field `k` of a `with`
    // on that record, outside any loop it is not in
    plans.retain(|t, (base, k, ..)| {
        let mut ok = true;
        let mut withs = 0;
        check(&f.body, *t, *base, *k, false, false, &mut ok, &mut withs);
        ok && withs == uses.get(t).copied().unwrap_or(0) && withs > 0
    });
    // (a chain whose start is another plan's: only the longest)
    let inner: Vec<Val> = plans.values().flat_map(|p| p.3[1..].to_vec()).collect();
    plans.retain(|t, _| !inner.contains(t));
    if plans.is_empty() {
        return;
    }
    let gone: Vec<Val> = plans.values().flat_map(|p| p.3.clone()).collect();
    apply(&mut f.body, &plans, &gone);
}

fn count(r: &Region, uses: &mut HashMap<Val, usize>) {
    for i in &r.insts {
        for v in i.op.operands() {
            *uses.entry(v).or_default() += 1;
        }
        for g in i.op.regions() {
            count(g, uses);
        }
    }
    if let Some(v) = r.result {
        *uses.entry(v).or_default() += 1;
    }
}

fn collect(r: &Region, defs: &mut HashMap<Val, Op>) {
    for i in &r.insts {
        defs.insert(i.dst, i.op.clone());
        for g in i.op.regions() {
            collect(g, defs);
        }
    }
}

/// Walks the uses of `t`: each must be field `k` of a `with` on `base` (and
/// only there in it), and not inside a loop entered after `t` is made.
/// `seen`: `t` is defined above this point; `looped`: a loop entered since.
#[allow(clippy::too_many_arguments)]
fn check(r: &Region, t: Val, base: Val, k: u32, seen: bool, looped: bool, ok: &mut bool, withs: &mut usize) {
    let mut seen = seen;
    if r.result == Some(t) {
        *ok = false;
    }
    for i in &r.insts {
        if i.dst == t {
            seen = true;
        }
        if i.op.operands().contains(&t) {
            match &i.op {
                Op::With { base: b, fields, .. } if *b == base && fields.iter().filter(|f| f.1 == t).count() == 1 && fields.iter().any(|f| f.1 == t && f.0 == k) && !looped && seen => *withs += 1,
                _ => *ok = false,
            }
        }
        let in_loop = matches!(i.op, Op::Loop { .. });
        for g in i.op.regions() {
            check(g, t, base, k, seen, looped || (in_loop && seen), ok, withs);
        }
    }
}

fn apply(r: &mut Region, plans: &HashMap<Val, (Val, u32, Vec<Change>, Vec<Val>)>, gone: &[Val]) {
    r.insts.retain(|i: &Inst| !gone.contains(&i.dst));
    for i in &mut r.insts {
        match &mut i.op {
            Op::With { fields, moves, changes, .. } => {
                let mut j = 0;
                while j < fields.len() {
                    if let Some((_, k, chain, _)) = plans.get(&fields[j].1) {
                        changes.push((*k, chain.clone()));
                        fields.remove(j);
                        moves.remove(j);
                    } else {
                        j += 1;
                    }
                }
            }
            Op::If { then, els, .. } => {
                apply(then, plans, gone);
                apply(els, plans, gone);
            }
            Op::Loop { done, step, .. } => {
                if let Some(d) = done {
                    apply(d, plans, gone);
                }
                apply(step, plans, gone);
            }
            _ => {}
        }
    }
}
