// allocation: build complete binary trees and count their nodes
// (64 trees of depth 16 in parallel, then one of depth 22 built with
// rayon::join down to a cut-off)
use rayon::prelude::*;

enum Tree {
    Leaf,
    Node(Box<Tree>, Box<Tree>),
}

fn make(d: u32) -> Tree {
    if d == 0 { Tree::Leaf } else { Tree::Node(Box::new(make(d - 1)), Box::new(make(d - 1))) }
}

fn pmake(d: u32) -> Tree {
    if d < 14 {
        return make(d);
    }
    let (l, r) = rayon::join(|| pmake(d - 1), || pmake(d - 1));
    Tree::Node(Box::new(l), Box::new(r))
}

fn check(t: &Tree) -> i64 {
    match t {
        Tree::Leaf => 1,
        Tree::Node(l, r) => 1 + check(l) + check(r),
    }
}

fn main() {
    let s: i64 = (0..64).into_par_iter().map(|_| check(&make(16))).sum();
    println!("{s}");
    println!("{}", check(&pmake(22)));
}
