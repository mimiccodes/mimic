# The reference for markdown (Python's re): 400,000 lines of a Markdown
# subset from splitmix64 (line k: a heading of 1 to 3 #, a "- " list item,
# a blank line, or a paragraph; items and paragraphs are words, some
# **bold**, some `code`, some [links](https://x.example/k)). Prints the
# headings at each level, the list items, the bold spans, the links, the
# code spans, and the words outside the markup (spaces apart).
import re

M = (1 << 64) - 1
WORDS = ["array", "kernel", "fusion", "memory", "vector", "thread", "cache", "loop"]


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def word(k, j):
    w = WORDS[mix(16 * k + j) % 8]
    r = mix(16 * k + j + 8) % 10
    return f"**{w}**" if r == 0 else f"`{w}`" if r == 1 else f"[{w}](https://x.example/{k})" if r == 2 else w


def line(k):
    r = mix(k + 10**8) % 10
    if r == 0:
        return "#" * (1 + mix(k) % 3) + " " + " ".join(WORDS[mix(k + j) % 8] for j in range(3))
    if r == 1:
        return ""
    body = " ".join(word(k, j) for j in range(2 + mix(k) % 8))
    return "- " + body if r < 4 else body


text = "\n".join(line(k) for k in range(400000))
lines = text.split("\n")
heads = [sum(1 for l in lines if re.match(r"#{%d} " % n, l)) for n in (1, 2, 3)]
items = sum(1 for l in lines if l.startswith("- "))
body = [re.sub(r"^(#+|-) ", "", l) for l in lines]
bold = sum(len(re.findall(r"\*\*\w+\*\*", l)) for l in body)
links = sum(len(re.findall(r"\[\w+\]\([^)]*\)", l)) for l in body)
code = sum(len(re.findall(r"`\w+`", l)) for l in body)
plain = sum(1 for l in body for w in l.split(" ") if re.fullmatch(r"\w+", w))
print(*heads, items, bold, links, code, plain)
