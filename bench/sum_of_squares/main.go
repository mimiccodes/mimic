package main

import "fmt"

func main() {
	var s int64
	for i := int64(0); i < 100000000; i++ {
		x := i % 1000
		s += x * x
	}
	fmt.Println(s)
}
