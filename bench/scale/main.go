// scale: 30 frames of 1920x1080 8-bit pixels, each scaled (bilinear, 8-bit
// fixed-point weights) to 1280x720, 854x480, 640x360 and 426x240; prints the
// sum of every output pixel
package main

import "fmt"

func frame(w, h, f int64) []uint8 {
	p := make([]uint8, w*h)
	for i := int64(0); i < w*h; i++ {
		x, y := i%w, i/w
		p[i] = uint8((x*7 + y*3 + x*y%13 + f*5) % 256)
	}
	return p
}

func scale(src []uint8, w, h, w2, h2 int64) []uint8 {
	dst := make([]uint8, w2*h2)
	for i := int64(0); i < w2*h2; i++ {
		sx := i % w2 * (w - 1) * 256 / (w2 - 1)
		sy := i / w2 * (h - 1) * 256 / (h2 - 1)
		x0, y0, fx, fy := sx>>8, sy>>8, sx&255, sy&255
		x1, y1 := min(x0+1, w-1), min(y0+1, h-1)
		top := int64(src[y0*w+x0])*(256-fx) + int64(src[y0*w+x1])*fx
		bot := int64(src[y1*w+x0])*(256-fx) + int64(src[y1*w+x1])*fx
		dst[i] = uint8((top*(256-fy) + bot*fy + 32768) >> 16)
	}
	return dst
}

func main() {
	sizes := [][2]int64{{1280, 720}, {854, 480}, {640, 360}, {426, 240}}
	var total int64
	for f := int64(0); f < 30; f++ {
		src := frame(1920, 1080, f)
		for _, s := range sizes {
			for _, b := range scale(src, 1920, 1080, s[0], s[1]) {
				total += int64(b)
			}
		}
	}
	fmt.Println(total)
}
