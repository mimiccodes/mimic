# The reference for siphash (pure Python): SipHash-2-4 with the key
# k0 = 0x0706050403020100, k1 = 0x0f0e0d0c0b0a0908 (bytes 0-15) over
# 2,000,000 messages of 16 bytes, message j being mix(2j) then mix(2j + 1)
# as little-endian 64-bit words (splitmix64). The implementation is checked
# first against the test vector of the SipHash paper (the 15 bytes 0-14
# hash to 0xa129ca6149be45e5). Prints the exclusive or of all the hashes and
# their sum modulo 1,000,000,007 (as unsigned numbers).
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def rotl(x, r):
    return (x << r | x >> (64 - r)) & M


def siphash(k0, k1, msg):
    v0, v1, v2, v3 = k0 ^ 0x736F6D6570736575, k1 ^ 0x646F72616E646F6D, k0 ^ 0x6C7967656E657261, k1 ^ 0x7465646279746573

    def rounds(n):
        nonlocal v0, v1, v2, v3
        for _ in range(n):
            v0 = (v0 + v1) & M; v1 = rotl(v1, 13) ^ v0; v0 = rotl(v0, 32)
            v2 = (v2 + v3) & M; v3 = rotl(v3, 16) ^ v2
            v0 = (v0 + v3) & M; v3 = rotl(v3, 21) ^ v0
            v2 = (v2 + v1) & M; v1 = rotl(v1, 17) ^ v2; v2 = rotl(v2, 32)

    n = len(msg)
    for i in range(0, n - n % 8, 8):
        m = int.from_bytes(msg[i:i + 8], "little")
        v3 ^= m
        rounds(2)
        v0 ^= m
    b = (n & 255) << 56 | int.from_bytes(msg[n - n % 8:], "little")
    v3 ^= b
    rounds(2)
    v0 ^= b
    v2 ^= 0xFF
    rounds(4)
    return v0 ^ v1 ^ v2 ^ v3


k0, k1 = 0x0706050403020100, 0x0F0E0D0C0B0A0908
assert siphash(k0, k1, bytes(range(15))) == 0xA129CA6149BE45E5
x = s = 0
for j in range(2000000):
    h = siphash(k0, k1, mix(2 * j).to_bytes(8, "little") + mix(2 * j + 1).to_bytes(8, "little"))
    x ^= h
    s = (s + h) % 1000000007
print(f"{x:016x}", s)
