# The reference for dither (pure Python): 24 frames of 1280×720 8-bit pixels,
# each dithered to black and white by Floyd–Steinberg error diffusion in
# raster order: a pixel of 128 or more becomes 255, else 0, and the error
# e = old - new goes on as e*7 >> 4 to the right, e*3 >> 4 below left,
# e*5 >> 4 below and e >> 4 below right (shifts floor, as in C and Rust;
# what would fall outside the image is dropped). Prints, per frame, the
# number of white pixels and a checksum that weighs every white pixel by
# (i mod 251 + 1).
W, H = 1280, 720
n = W * H
for f in range(24):
    buf = []
    for i in range(n):
        x, y = i % W, i // W
        noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 // 65536 % 32
        buf.append((x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256)
    white = check = 0
    for i in range(n):
        old = buf[i]
        new = 255 if old >= 128 else 0
        e = old - new
        x = i % W
        if x + 1 < W:
            buf[i + 1] += e * 7 >> 4
        if i + W < n:
            if x > 0:
                buf[i + W - 1] += e * 3 >> 4
            buf[i + W] += e * 5 >> 4
            if x + 1 < W:
                buf[i + W + 1] += e >> 4
        if new:
            white += 1
            check += i % 251 + 1
    print(f, white, check)
