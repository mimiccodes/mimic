# The reference for template (Python's str.replace): the template
# "Dear {{name}}, your order {{id}} of {{qty}} x {{item}} ships on {{day}}.
# Total: {{total}}." rendered 300,000 times with values from splitmix64
# (render r: name "user" + r mod 1000, id r, qty 1 + mix(r) mod 9, item one
# of five words by mix(r + 1) mod 5, day one of seven by mix(r + 2) mod 7,
# total qty × (100 + mix(r + 3) mod 900)). Prints the total length of the
# rendered texts (in characters) and the sum of their characters' codes.
M = (1 << 64) - 1
ITEMS = ["lamp", "kettle", "chair", "notebook", "umbrella"]
DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
TEMPLATE = "Dear {{name}}, your order {{id}} of {{qty}} x {{item}} ships on {{day}}. Total: {{total}}."


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


length = codes = 0
for r in range(300000):
    qty = 1 + mix(r) % 9
    vals = {"name": f"user{r % 1000}", "id": str(r), "qty": str(qty), "item": ITEMS[mix(r + 1) % 5],
            "day": DAYS[mix(r + 2) % 7], "total": str(qty * (100 + mix(r + 3) % 900))}
    out = TEMPLATE
    for k, v in vals.items():
        out = out.replace("{{" + k + "}}", v)
    length += len(out)
    codes += sum(map(ord, out))
print(length, codes)
