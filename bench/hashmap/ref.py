# The reference for hashmap (Python): 1,000,000 keys (1 + mix(i) % 700,000,
# from splitmix64) go into a table, each adding i % 100 to its value; then
# keys 1 + q % 1,400,000 for q below 2,000,000 are looked up. Prints the sum
# of the values found, how many lookups found their key, and how many
# distinct keys went in (the slots in use).
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 1000000
table = {}
for i in range(n):
    k = 1 + mix(i) % 700000
    table[k] = table.get(k, 0) + i % 100
total = found = 0
for q in range(2 * n):
    k = 1 + q % 1400000
    if k in table:
        total += table[k]
        found += 1
print(total, found, len(table))
