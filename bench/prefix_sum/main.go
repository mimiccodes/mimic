package main

import "fmt"

func main() {
	const n = 100000000
	s := make([]int64, n)
	var acc int64
	for i := int64(0); i < n; i++ {
		acc += i % 1000
		s[i] = acc
	}
	fmt.Println(s[n/2], s[n-1])
}
