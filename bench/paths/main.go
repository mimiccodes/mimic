// paths: Dijkstra and A* on a 600x600 grid with a binary heap (lazy
// deletion); prints each search's cost and nodes expanded, and the path's
// length (see main.c)
package main

import "fmt"

const W, H = 600, 600
const N = W * H
const FAR = int64(1) << 40

func est(v int64, astar bool) int64 {
	if astar {
		return (W - 1 - v%W) + (H - 1 - v/W)
	}
	return 0
}

func search(cost []int64, astar bool) (int64, int64, []int64) {
	dist, prev, closed := make([]int64, N), make([]int64, N), make([]bool, N)
	for i := range dist {
		dist[i], prev[i] = FAR, -1
	}
	heap := []int64{est(0, astar) * N}
	dist[0] = 0
	var expanded int64
	for len(heap) > 0 {
		top := heap[0]
		last := heap[len(heap)-1]
		heap = heap[:len(heap)-1]
		if len(heap) > 0 {
			heap[0] = last
			for k := 0; ; {
				l, r, m := 2*k+1, 2*k+2, k
				if l < len(heap) && heap[l] < heap[m] {
					m = l
				}
				if r < len(heap) && heap[r] < heap[m] {
					m = r
				}
				if m == k {
					break
				}
				heap[k], heap[m] = heap[m], heap[k]
				k = m
			}
		}
		u := top % N
		if closed[u] {
			continue
		}
		closed[u] = true
		expanded++
		if u == N-1 {
			break
		}
		x, y := u%W, u/W
		nb := [4]int64{-1, -1, -1, -1}
		if x > 0 {
			nb[0] = u - 1
		}
		if x < W-1 {
			nb[1] = u + 1
		}
		if y > 0 {
			nb[2] = u - W
		}
		if y < H-1 {
			nb[3] = u + W
		}
		for _, v := range nb {
			if v < 0 || cost[v] == 0 || closed[v] {
				continue
			}
			d := dist[u] + cost[v]
			if d < dist[v] {
				dist[v], prev[v] = d, u
				heap = append(heap, (d+est(v, astar))*N+v)
				for k := len(heap) - 1; k > 0 && heap[k] < heap[(k-1)/2]; k = (k - 1) / 2 {
					heap[k], heap[(k-1)/2] = heap[(k-1)/2], heap[k]
				}
			}
		}
	}
	return dist[N-1], expanded, prev
}

func main() {
	cost := make([]int64, N)
	for i := int64(0); i < N; i++ {
		x, y := i%W, i/W
		if x%50 == 25 && y%100 != 10 && y%100 != 60 {
			cost[i] = 0
		} else {
			cost[i] = (i*1103515245+12345)%2147483648/65536%9 + 1
		}
	}
	dc, de, _ := search(cost, false)
	fmt.Println("dijkstra", dc, de)
	ac, ae, prev := search(cost, true)
	fmt.Println("astar", ac, ae)
	cells := int64(1)
	for v := int64(N - 1); prev[v] >= 0; v = prev[v] {
		cells++
	}
	fmt.Println("path", cells)
}
