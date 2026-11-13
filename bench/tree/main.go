package main

import "fmt"

// allocation: build complete binary trees and count their nodes
// (64 trees of depth 16, then one of depth 22)
type Tree struct{ l, r *Tree }

func make_(d int) *Tree {
	if d == 0 {
		return &Tree{}
	}
	return &Tree{make_(d - 1), make_(d - 1)}
}

func check(t *Tree) int64 {
	if t.l == nil {
		return 1
	}
	return 1 + check(t.l) + check(t.r)
}

func main() {
	var s int64
	for i := 0; i < 64; i++ {
		s += check(make_(16))
	}
	fmt.Println(s)
	fmt.Println(check(make_(22)))
}
