# The reference for sha1 (Python's hashlib): SHA-1 of 4 MiB of generated
# bytes (byte i: (i * 2654435761 >> 13) & 255). Prints the digest in
# hexadecimal.
import hashlib

n = 1 << 22
print(hashlib.sha1(bytes(((i * 2654435761) >> 13) & 255 for i in range(n))).hexdigest())
