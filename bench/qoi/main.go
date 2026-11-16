// qoi: a 1024x768 RGBA image encoded as QOI (qoiformat.org) and decoded
// again, 5 times; prints the encoded size, a checksum of the bytes and
// whether the decoded pixels equal the original (see main.c)
package main

import "fmt"

const W, H = 1024, 768

func rgba(r, g, b, a uint32) uint32 { return r<<24 | g<<16 | b<<8 | a }
func slot(p uint32) uint32         { return ((p>>24)*3 + (p>>16&255)*5 + (p>>8&255)*7 + (p&255)*11) % 64 }

func pixel(i int64) uint32 {
	x, y := i%W, i/W
	noise := (i*1103515245 + 12345) % 2147483648 / 65536
	band := y * 10 / H
	switch {
	case band < 2:
		return rgba(40, 90, 200, 255)
	case band < 5:
		return rgba(uint32(x*3%256), uint32(y*5%256), uint32((x+y)%256), 255)
	case band < 7:
		return []uint32{0xff0000ff, 0x00ff00ff, 0x0000ffff}[(x/5+y)%3]
	case band < 9:
		return rgba(uint32(noise%256), uint32(noise/256%256), uint32(noise/7%256), 255)
	}
	return rgba(uint32((120+x)%256), 60, uint32((30+y)%256), uint32(x*4%256))
}

func encode(px []uint32) []uint8 {
	out := []uint8{'q', 'o', 'i', 'f', 0, 0, W >> 8, W & 255, 0, 0, H >> 8, H & 255, 4, 0}
	var seen [64]uint32
	prev, run := rgba(0, 0, 0, 255), uint8(0)
	for i, p := range px {
		if p == prev {
			run++
			if run == 62 || i == len(px)-1 {
				out = append(out, 192|(run-1))
				run = 0
			}
			continue
		}
		if run > 0 {
			out = append(out, 192|(run-1))
			run = 0
		}
		h := slot(p)
		if seen[h] == p {
			out = append(out, uint8(h))
		} else {
			seen[h] = p
			if p&255 == prev&255 {
				d := func(s uint32) int32 { return int32(int8(uint8(p>>s) - uint8(prev>>s))) }
				dr, dg, db := d(24), d(16), d(8)
				drdg, dbdg := dr-dg, db-dg
				if dr > -3 && dr < 2 && dg > -3 && dg < 2 && db > -3 && db < 2 {
					out = append(out, uint8(64|(dr+2)<<4|(dg+2)<<2|(db+2)))
				} else if drdg > -9 && drdg < 8 && dg > -33 && dg < 32 && dbdg > -9 && dbdg < 8 {
					out = append(out, uint8(128|(dg+32)), uint8((drdg+8)<<4|(dbdg+8)))
				} else {
					out = append(out, 254, uint8(p>>24), uint8(p>>16), uint8(p>>8))
				}
			} else {
				out = append(out, 255, uint8(p>>24), uint8(p>>16), uint8(p>>8), uint8(p))
			}
		}
		prev = p
	}
	return append(out, 0, 0, 0, 0, 0, 0, 0, 1)
}

func decode(d []uint8, n int) []uint32 {
	px := make([]uint32, 0, n)
	var seen [64]uint32
	prev, pos := rgba(0, 0, 0, 255), 14
	ch := func(v uint32, s uint32) int32 { return int32(v >> s & 255) }
	for len(px) < n {
		b := d[pos]
		var p uint32
		switch {
		case b == 254:
			p = rgba(uint32(d[pos+1]), uint32(d[pos+2]), uint32(d[pos+3]), prev&255)
			pos += 4
		case b == 255:
			p = rgba(uint32(d[pos+1]), uint32(d[pos+2]), uint32(d[pos+3]), uint32(d[pos+4]))
			pos += 5
		case b>>6 == 0:
			p = seen[b]
			pos++
		case b>>6 == 1:
			f := func(s uint32, k uint8) uint32 { return uint32((ch(prev, s) + int32(k&3) - 2) & 255) }
			p = rgba(f(24, b>>4), f(16, b>>2), f(8, b), prev&255)
			pos++
		case b>>6 == 2:
			dg := int32(b&63) - 32
			m := int32(d[pos+1])
			p = rgba(uint32((ch(prev, 24)+dg+(m>>4)-8)&255), uint32((ch(prev, 16)+dg)&255), uint32((ch(prev, 8)+dg+(m&15)-8)&255), prev&255)
			pos += 2
		default:
			for k := 0; k < int(b&63)+1; k++ {
				px = append(px, prev)
			}
			pos++
			continue
		}
		seen[slot(p)] = p
		px = append(px, p)
		prev = p
	}
	return px
}

func main() {
	px := make([]uint32, W*H)
	for i := range px {
		px[i] = pixel(int64(i))
	}
	var data []uint8
	ok := true
	for r := 0; r < 5; r++ {
		data = encode(px)
		back := decode(data, len(px))
		for i := range px {
			ok = ok && back[i] == px[i]
		}
	}
	var check int64
	for i, b := range data {
		check += int64(b) * (int64(i)%251 + 1)
	}
	fmt.Printf("%d bytes\n%d\n%v\n", len(data), check, ok)
}
