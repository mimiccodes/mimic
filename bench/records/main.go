// records: a million records with a name and numbers, updated five times
// (in place), then summed
package main

import "fmt"

type P struct {
	id   int64
	name string
	x, v float64
}

func main() {
	ps := make([]P, 1000000)
	for i := range ps {
		ps[i] = P{int64(i), "p", float64(i), 1.0}
	}
	for s := 0; s < 5; s++ {
		for i := range ps {
			ps[i].x += ps[i].v
			ps[i].v *= 0.5
		}
	}
	sx, se := 0.0, 0.0
	for _, p := range ps {
		sx += p.x
	}
	for _, p := range ps {
		se += p.v * p.v * 1024.0
	}
	fmt.Printf("%.1f\n%.1f\n", sx, se)
}
