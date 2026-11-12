package main

import "fmt"

func main() {
	const n, limit = 2000, 200
	count := 0
	for y := 0; y < n; y++ {
		for x := 0; x < n; x++ {
			cr := -2.0 + 2.5*float64(x)/float64(n)
			ci := -1.25 + 2.5*float64(y)/float64(n)
			zr, zi, k := 0.0, 0.0, 0
			for k < limit && zr*zr+zi*zi <= 4.0 {
				zr, zi = zr*zr-zi*zi+cr, 2.0*zr*zi+ci
				k++
			}
			if k == limit {
				count++
			}
		}
	}
	fmt.Println(count)
}
