//! Fusion: a list made by a map (`let ys = xs × 2`) that only one later
//! map or reduction in the same region reads, once, is never built: its
//! computation moves into that reader's, so the two are one loop
//! (`ys ▷ +/` sums `xs × 2` element by element).
//!
//! Only within a region (a loop's step reads its own values each time;
//! moving a computation into a branch or a loop would change how often it
//! runs). A map read more than once moves into each reader when it is
//! small (at most [`SMALL`] operations: computing it again is cheaper than
//! building it) and every read is by a map or reduction of the region;
//! otherwise it is built once. (The old path's lazy lets, the same rule.)

/// The most operations a map read several times may have and still be
/// computed again where each reads it.
const SMALL: usize = 8;

use crate::{Function, Op, Region, Tree, Val};

/// Fuses what can be fused in the whole function.
pub fn function(f: &mut Function) {
    let mut uses = vec![0u32; f.values as usize];
    count(&f.body, &mut uses);
    region(&mut f.body, &uses);
}

fn region(r: &mut Region, uses: &[u32]) {
    // until nothing moves: a chain `a`, `b = a × 2`, `c = b + 1` folds
    // into its last reader one step at a time
    while fuse_one(r, uses) {}
    for inst in &mut r.insts {
        match &mut inst.op {
            Op::If { then, els, .. } => {
                region(then, uses);
                region(els, uses);
            }
            Op::Loop { done, step, .. } => {
                if let Some(d) = done {
                    region(d, uses);
                }
                region(step, uses);
            }
            _ => {}
        }
    }
}

/// Moves one map into its only reader; whether one moved.
fn fuse_one(r: &mut Region, uses: &[u32]) -> bool {
    for j in 0..r.insts.len() {
        let mut leaves = Vec::new();
        match &r.insts[j].op {
            Op::Map(t) | Op::Reduce(_, t, _) => leaf_vals(t, &mut leaves),
            _ => continue,
        }
        for v in leaves {
            let n = uses.get(v.0 as usize).copied().unwrap_or(0);
            if n == 0 || r.result == Some(v) {
                continue;
            }
            let Some(i) = r.insts[..j].iter().position(|x| x.dst == v && matches!(x.op, Op::Map(_))) else { continue };
            let Op::Map(t) = &r.insts[i].op else { unreachable!() };
            // read once; or small, with every read a leaf of a map or a
            // reduction after it in this region
            if n > 1 {
                let leaf_reads: u32 = r.insts[i + 1..]
                    .iter()
                    .map(|x| match &x.op {
                        Op::Map(t) | Op::Reduce(_, t, _) => {
                            let mut ls = Vec::new();
                            leaf_vals(t, &mut ls);
                            ls.iter().filter(|w| **w == v).count() as u32
                        }
                        _ => 0,
                    })
                    .sum();
                if size(t) > SMALL || leaf_reads != n {
                    continue;
                }
            }
            let Op::Map(t) = r.insts.remove(i).op else { unreachable!() };
            for x in &mut r.insts[i..] {
                if let Op::Map(into) | Op::Reduce(_, into, _) = &mut x.op {
                    replace_leaf(into, v, &t);
                }
            }
            return true;
        }
    }
    false
}

/// How many operations a tree is.
fn size(t: &Tree) -> usize {
    match t {
        Tree::Bin(_, a, b) => 1 + size(a) + size(b),
        Tree::Un(_, a) => 1 + size(a),
        _ => 1,
    }
}

/// The lists a tree reads element by element.
fn leaf_vals(t: &Tree, out: &mut Vec<Val>) {
    match t {
        Tree::Leaf(v, _) => out.push(*v),
        Tree::Bin(_, a, b) => {
            leaf_vals(a, out);
            leaf_vals(b, out);
        }
        Tree::Un(_, a) => leaf_vals(a, out),
        _ => {}
    }
}

fn replace_leaf(t: &mut Tree, v: Val, with: &Tree) {
    match t {
        Tree::Leaf(w, _) if *w == v => *t = with.clone(),
        Tree::Bin(_, a, b) => {
            replace_leaf(a, v, with);
            replace_leaf(b, v, with);
        }
        Tree::Un(_, a) => replace_leaf(a, v, with),
        _ => {}
    }
}

/// How many times each value is read (a region's result counts as a read).
fn count(r: &Region, uses: &mut [u32]) {
    let bump = |v: Val, uses: &mut [u32]| {
        if let Some(n) = uses.get_mut(v.0 as usize) {
            *n += 1;
        }
    };
    for i in &r.insts {
        match &i.op {
            Op::Int(_) | Op::Float(_) | Op::Bool(_) | Op::Unit | Op::Param(_) => {}
            Op::Bin(_, a, b) | Op::Index(a, b, _) | Op::Set(a, _, b) => {
                bump(*a, uses);
                bump(*b, uses);
            }
            Op::Un(_, a) | Op::Get(a, _) | Op::Len(a) | Op::Drop(a) => bump(*a, uses),
            Op::Call { args, .. } => args.iter().for_each(|a| bump(*a, uses)),
            Op::Make(xs) => xs.iter().for_each(|a| bump(*a, uses)),
            Op::Embed { uses: us, .. } => us.iter().for_each(|(_, a, _)| bump(*a, uses)),
            Op::Map(t) | Op::Reduce(_, t, _) => {
                let mut vs = Vec::new();
                t.vals(&mut vs);
                vs.into_iter().for_each(|a| bump(a, uses));
            }
            Op::If { cond, then, els } => {
                bump(*cond, uses);
                count(then, uses);
                count(els, uses);
            }
            Op::Loop { init, count: n, done, step, .. } => {
                bump(*init, uses);
                if let Some(n) = n {
                    bump(*n, uses);
                }
                if let Some(d) = done {
                    count(d, uses);
                }
                count(step, uses);
            }
        }
    }
    if let Some(v) = r.result {
        bump(v, uses);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Function, Inst, Op, Region, Tree, Ty, Val};
    use mimic_check::types::{El, Ty as C};
    use mimic_syntax::ast::BinOp;
    use mimic_syntax::glyphs::Prim;

    fn inst(dst: u32, ty: Ty, op: Op) -> Inst {
        Inst { dst: Val(dst), ty, op, span: Default::default() }
    }

    fn list() -> Ty {
        Ty::List(C::Arr(El::I64, 0))
    }

    fn func(insts: Vec<Inst>, result: u32) -> Function {
        Function { name: "t".into(), params: vec![("n".into(), Ty::I64)], ret: Ty::I64, body: Region { insts, result: Some(Val(result)) }, values: 10 }
    }

    #[test]
    fn a_map_read_once_moves_into_its_reader() {
        // %1 = map ⍳%0 × 2; %2 = reduce Sum %1[i]
        let mut f = func(
            vec![
                inst(0, Ty::I64, Op::Param(0)),
                inst(1, list(), Op::Map(Tree::Bin(BinOp::Mul, Box::new(Tree::Iota(Val(0))), Box::new(Tree::Int(2))))),
                inst(2, Ty::I64, Op::Reduce(Prim::Sum, Tree::Leaf(Val(1), El::I64), El::I64)),
            ],
            2,
        );
        super::function(&mut f);
        assert_eq!(f.body.insts.len(), 2);
        let Op::Reduce(_, Tree::Bin(BinOp::Mul, a, _), _) = &f.body.insts[1].op else { panic!("not fused: {:?}", f.body.insts[1].op) };
        assert!(matches!(**a, Tree::Iota(Val(0))));
    }

    #[test]
    fn a_small_map_read_twice_is_computed_twice() {
        let mut f = func(
            vec![
                inst(0, Ty::I64, Op::Param(0)),
                inst(1, list(), Op::Map(Tree::Iota(Val(0)))),
                inst(2, Ty::I64, Op::Reduce(Prim::Sum, Tree::Bin(BinOp::Mul, Box::new(Tree::Leaf(Val(1), El::I64)), Box::new(Tree::Leaf(Val(1), El::I64))), El::I64)),
            ],
            2,
        );
        super::function(&mut f);
        assert_eq!(f.body.insts.len(), 2);
    }

    #[test]
    fn a_map_also_read_otherwise_is_built() {
        // %1 = map ⍳%0; %2 = reduce Sum %1[i]; %3 = len %1
        let mut f = func(
            vec![
                inst(0, Ty::I64, Op::Param(0)),
                inst(1, list(), Op::Map(Tree::Iota(Val(0)))),
                inst(2, Ty::I64, Op::Reduce(Prim::Sum, Tree::Leaf(Val(1), El::I64), El::I64)),
                inst(3, Ty::I64, Op::Len(Val(1))),
                inst(4, Ty::I64, Op::Bin(BinOp::Add, Val(2), Val(3))),
            ],
            4,
        );
        super::function(&mut f);
        assert_eq!(f.body.insts.len(), 5);
    }

    #[test]
    fn a_big_map_read_twice_is_built() {
        let big = (0..9).fold(Tree::Iota(Val(0)), |t, k| Tree::Bin(BinOp::Add, Box::new(t), Box::new(Tree::Int(k))));
        let mut f = func(
            vec![
                inst(0, Ty::I64, Op::Param(0)),
                inst(1, list(), Op::Map(big)),
                inst(2, Ty::I64, Op::Reduce(Prim::Sum, Tree::Bin(BinOp::Mul, Box::new(Tree::Leaf(Val(1), El::I64)), Box::new(Tree::Leaf(Val(1), El::I64))), El::I64)),
            ],
            2,
        );
        super::function(&mut f);
        assert_eq!(f.body.insts.len(), 3);
    }
}
