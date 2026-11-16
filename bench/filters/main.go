// filters: a 1920x1080 8-bit image through five filters (edges clamped):
// 3x3 box blur, 5x5 Gaussian, Sobel, 3x3 median, integer bilateral; prints
// each result's sum and a position-weighted checksum (see main.c)
package main

import (
	"fmt"
	"sort"
)

const W, H = 1920, 1080

var img = make([]uint8, W*H)

func at(x, y int64) int64 {
	x, y = max(0, min(x, W-1)), max(0, min(y, H-1))
	return int64(img[y*W+x])
}

func abs(v int64) int64 {
	if v < 0 {
		return -v
	}
	return v
}

func filter(f func(x, y, i int64) int64) []uint8 {
	out := make([]uint8, W*H)
	for i := int64(0); i < W*H; i++ {
		out[i] = uint8(f(i%W, i/W, i))
	}
	return out
}

func report(name string, out []uint8) {
	var sum, check int64
	for i, b := range out {
		sum += int64(b)
		check += int64(b) * (int64(i)%251 + 1)
	}
	fmt.Println(name, sum, check)
}

func main() {
	for i := int64(0); i < W*H; i++ {
		x, y := i%W, i/W
		noise := (i*1103515245 + 12345) % 2147483648 / 65536 % 32
		img[i] = uint8((x*7 + y*13 + x*y%31 + noise) % 256)
	}
	report("box", filter(func(x, y, _ int64) int64 {
		var s int64
		for j := int64(0); j < 9; j++ {
			s += at(x+j%3-1, y+j/3-1)
		}
		return s / 9
	}))
	g := []int64{1, 4, 6, 4, 1}
	report("gaussian", filter(func(x, y, _ int64) int64 {
		var s int64
		for j := int64(0); j < 25; j++ {
			s += g[j/5] * g[j%5] * at(x+j%5-2, y+j/5-2)
		}
		return s / 256
	}))
	sx, sy := []int64{-1, 0, 1, -2, 0, 2, -1, 0, 1}, []int64{-1, -2, -1, 0, 0, 0, 1, 2, 1}
	report("sobel", filter(func(x, y, _ int64) int64 {
		var a, b int64
		for j := int64(0); j < 9; j++ {
			p := at(x+j%3-1, y+j/3-1)
			a += sx[j] * p
			b += sy[j] * p
		}
		return min(abs(a)+abs(b), 255)
	}))
	report("median", filter(func(x, y, _ int64) int64 {
		v := make([]int64, 9)
		for j := int64(0); j < 9; j++ {
			v[j] = at(x+j%3-1, y+j/3-1)
		}
		sort.Slice(v, func(a, b int) bool { return v[a] < v[b] })
		return v[4]
	}))
	sw := []int64{1, 2, 1, 2, 4, 2, 1, 2, 1}
	report("bilateral", filter(func(x, y, i int64) int64 {
		p := int64(img[i])
		var num, den int64
		for j := int64(0); j < 9; j++ {
			q := at(x+j%3-1, y+j/3-1)
			w := sw[j] * max(32-abs(q-p)/2, 0)
			num += w * q
			den += w
		}
		return (num + den/2) / den
	}))
}
