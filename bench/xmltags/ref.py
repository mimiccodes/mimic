# The reference for xmltags (Python's xml.etree.ElementTree): an XML
# document of 300,000 elements under a root <doc>, written by a walk that
# at step k opens an element (tag one of item, group, note, ref, entry,
# chosen by mix(k) mod 5; attribute id="k"; text "t") when mix(k + 10^7)
# mod 3 is not 0 and the depth is below 12, else closes one (splitmix64).
# Prints, for the five tags in that order, how many elements have them, then
# the deepest nesting (the root at depth 1) and the sum of the ids.
import sys
import xml.etree.ElementTree as ET

M = (1 << 64) - 1
TAGS = ["item", "group", "note", "ref", "entry"]


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


parts, stack, made, k = ["<doc>"], [], 0, 0
while made < 300000:
    if mix(k + 10**7) % 3 != 0 and len(stack) < 11 or not stack:
        tag = TAGS[mix(k) % 5]
        parts.append(f'<{tag} id="{k}">t')
        stack.append(tag)
        made += 1
    else:
        parts.append(f"</{stack.pop()}>")
    k += 1
parts += [f"</{t}>" for t in reversed(stack)] + ["</doc>"]
text = "".join(parts)
open(sys.argv[1], "w").write(text) if len(sys.argv) > 1 else None
root = ET.fromstring(text)
counts = {t: 0 for t in TAGS}
ids, deepest = 0, 1


def walk(e, d):
    global ids, deepest
    deepest = max(deepest, d)
    for c in e:
        counts[c.tag] += 1
        ids += int(c.get("id"))
        walk(c, d + 1)


walk(root, 1)
print(*[counts[t] for t in TAGS], deepest, ids)
