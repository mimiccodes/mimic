package main

import (
	"fmt"
	"slices"
)

func main() {
	const n = 10000000
	xs := make([]int64, n)
	for i := int64(0); i < n; i++ {
		xs[i] = (i*1103515245 + 12345) % 2147483648
	}
	slices.Sort(xs)
	fmt.Println(xs[0], xs[n/2], xs[n-1])
}
