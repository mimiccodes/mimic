package main

import "fmt"

func main() {
	const n = 1024
	a := make([]float64, n*n)
	b := make([]float64, n*n)
	c := make([]float64, n*n)
	for i := 0; i < n; i++ {
		for j := 0; j < n; j++ {
			a[i*n+j] = float64((i * j) % 7)
			b[i*n+j] = float64((i + 2*j) % 5)
		}
	}
	for i := 0; i < n; i++ {
		for k := 0; k < n; k++ {
			x := a[i*n+k]
			for j := 0; j < n; j++ {
				c[i*n+j] += x * b[k*n+j]
			}
		}
	}
	s := 0.0
	for _, v := range c {
		s += v
	}
	fmt.Printf("%.1f\n", s)
}
