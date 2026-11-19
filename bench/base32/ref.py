# The reference for base32 (Python's base64 module): 4,000,000 generated
# bytes (byte i: (i * 2654435761 >> 13) & 255) encoded as Base32 (RFC
# 4648: groups of 5 bytes become 8 letters of A-Z2-7; 4,000,000 bytes are
# whole groups, so no padding) and decoded again. Prints the length of the
# text, a checksum that weighs every letter by (i mod 251 + 1), and 1 when
# decoding gives the bytes back.
import base64

n = 4000000
data = bytes(((i * 2654435761) >> 13) & 255 for i in range(n))
text = base64.b32encode(data)
print(len(text), sum(c * (i % 251 + 1) for i, c in enumerate(text)), 1 if base64.b32decode(text) == data else 0)
