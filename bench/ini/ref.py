# The reference for ini (Python's configparser): an INI file of 100,000
# sections. Section s is [server.s] with the keys host, port, timeout,
# enabled and name (values from splitmix64), in an order that varies, with a
# comment line (; or #) every few sections and blank lines between. Read
# back, it prints the number of sections, the number of keys, the sum of the
# ports, how many sections are enabled, the total length of the names, and
# the sum of each port times its section's place (1, 2, …) in the file.
import configparser

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def section(s):
    vals = [
        f"host = h{mix(s) % 1000}.example.com",
        f"port = {1024 + mix(s + 1) % 60000}",
        f"timeout = {mix(s + 2) % 120}",
        f"enabled = {'true' if mix(s + 3) % 3 else 'false'}",
        f"name = {'x' * (1 + mix(s + 4) % 20)}",
    ]
    r = mix(s + 5) % 5
    vals = vals[r:] + vals[:r]
    head = [f"; section {s}"] if s % 7 == 0 else ["# next"] if s % 11 == 0 else []
    return "\n".join(head + [f"[server.{s}]"] + vals + [""])


text = "\n".join(section(s) for s in range(100000))
cp = configparser.ConfigParser(comment_prefixes=(";", "#"), interpolation=None)
cp.read_string(text)
secs = cp.sections()
keys = sum(len(cp[s]) for s in secs)
print(len(secs), keys, sum(cp.getint(s, "port") for s in secs), sum(1 for s in secs if cp.getboolean(s, "enabled")),
      sum(len(cp[s]["name"]) for s in secs), sum(cp.getint(s, "port") * (k + 1) for k, s in enumerate(secs)))
