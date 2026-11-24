# The held-out set

Programs written plainly, the way someone who has just read the tutorial
would write them, each in Mimic and in C with the output both must print
(`expected.txt`). They measure whether Mimic is fast on programs nobody
tuned the compiler on; the corpus in `bench/` measures the rest.

The rules (docs/versions.md, stage 0.4):

* **Written independently.** The programs were written from the task list
  below with the tutorial (`docs/tutorial/`) as the only reference: not the
  compiler's source, not `docs/gaps.md`, not the corpus. Mimic was run
  only to check the output, never timed while writing.
* **Frozen.** A program here is never changed to make it faster. A fix is
  allowed only when it prints the wrong result or does not compile, and
  the fix keeps its style.
* **Never used for tuning.** A slow spot found here gets a corpus program
  of its own in `bench/` to tune on, and a row in `docs/gaps.md` with the
  0.4 step that closes it.
* **Measured at every 0.4 release** on Linux, macOS and Windows:
  `bench/run.py --heldout`. Stage 0.4 closes when Mimic is within 1.5× of
  C on one core (geometric mean), with no program above 3×.

## Tasks

Each program makes its own input from a fixed seed with the generator
`x = (x * 1103515245 + 12345) % 2147483648` (starting from `x = 42`
unless the task says otherwise), sized so the C version takes roughly
0.1 to 0.5 s, and prints a few numbers that check the work.

| # | Name | Task |
|---|---|---|
| 1 | `wordfreq` | Build a text of 2,000,000 words drawn from a list of 50 words; count each word; print the 10 most frequent with their counts. |
| 2 | `caesar` | Encrypt a generated 10 MB text with a Caesar shift of 13 (letters only), decrypt it again, print a checksum of the cipher text and whether the round trip matches. |
| 3 | `palindromes` | Count the palindromic substrings of a generated 3,000-letter string over `ab` by expanding around each centre. |
| 4 | `csvsum` | Build CSV text of 300,000 rows of 4 numeric columns, parse it back, print each column's sum and the row with the largest third column. |
| 5 | `brackets` | Generate 200,000 strings of brackets `()[]{}`; check each for balance with a stack; print how many are balanced and the deepest nesting seen. |
| 6 | `rle` | Run-length encode a generated 5 MB string of few distinct letters, decode it, print the encoded length and whether the round trip matches. |
| 7 | `reversewords` | Reverse the order of words in each of 300,000 generated sentences; print a checksum of the result. |
| 8 | `primes` | Count the primes below 3,000,000 by trial division up to the square root. |
| 9 | `gcdsum` | Sum gcd(i, j) for all 1 ≤ i, j ≤ 2,000 with Euclid's algorithm. |
| 10 | `perfect` | Find the perfect numbers and the abundant-number count below 300,000 by summing divisors. |
| 11 | `digitsum` | For n below 20,000,000 sum the digit sums of n, and count the n whose digit sum is 30. |
| 12 | `matmul` | Multiply two 300 × 300 matrices of floats held as lists of rows; print the trace and the sum of all entries. |
| 13 | `newton` | Square roots of 5,000,000 numbers by Newton's method (to 1e-12), summed. |
| 14 | `mandel` | Count the points of a 1,200 × 800 grid in the Mandelbrot set (at most 200 iterations). |
| 15 | `bigfactorial` | 3,000! with a list of digits (schoolbook multiplication); print its digit sum and number of digits. |
| 16 | `bfsgrid` | Shortest path by breadth-first search on a 1,000 × 1,000 grid with generated walls, start top-left, goal bottom-right. |
| 17 | `bst` | Insert 300,000 generated keys into a binary search tree (records or a union type), then count the keys in 10,000 ranges by walking it; print the totals and the height. |
| 18 | `linkedlist` | Build a singly linked list of 1,000,000 nodes as a union type, reverse it, sum every third value. |
| 19 | `heap` | A binary heap written by hand: push 1,000,000 generated numbers, pop them all, print a checksum of the popped order. |
| 20 | `unionfind` | Union-find with path compression over 1,000,000 elements and 2,000,000 generated unions; print the number of sets and the largest one. |
| 21 | `insertionsort` | Sort 20,000 generated numbers with a hand-written insertion sort; print a checksum. |
| 22 | `mergesort` | A hand-written recursive merge sort of 2,000,000 numbers; print a checksum and whether it is sorted. |
| 23 | `quicksort` | A hand-written quicksort of 2,000,000 numbers; print a checksum. |
| 24 | `life` | Conway's Game of Life on a 300 × 300 grid held as a list of rows, 300 generations; print the live cells. |
| 25 | `bank` | 500 accounts as records; apply 2,000,000 generated deposits, withdrawals (refused when short) and transfers; print the total, the richest account and the refusals. |
| 26 | `particles` | 2,000 particles (records of position and velocity) moving in a box with walls for 1,000 steps; print the sum of positions. |
| 27 | `inventory` | A map from item name to a record (count, price); apply 1,000,000 generated orders and restocks; print the stock value and the 5 lowest counts. |
| 28 | `knapsack` | 0/1 knapsack, 200 items, capacity 20,000, by dynamic programming over a table; print the best value. |
| 29 | `editdistance` | Edit distance between 2,000 pairs of generated 100-letter strings; print the total. |
| 30 | `dijkstra` | Dijkstra on a generated graph of 20,000 nodes and 100,000 weighted edges held as adjacency lists; print the sum of distances from node 0. |
| 31 | `toposort` | Topological order of a generated DAG of 200,000 nodes and 600,000 edges; print a checksum of the order. |
| 32 | `nqueens` | Count the solutions of 11 queens by backtracking. |
| 33 | `sudokucheck` | Generate 200,000 9 × 9 grids (shifted rows of a valid one, some with a swapped cell) and check each is a valid solution; print how many are. |
| 34 | `histogram` | Bucket 20,000,000 generated values into 100 bins; print the 3 fullest bins. |
| 35 | `movingavg` | A 50-wide moving average over 10,000,000 generated floats; print the maximum and the last value. |
| 36 | `textstats` | For a generated 5 MB text: lines, words, characters, the longest line, and the most common letter. |
| 37 | `tokenizer` | Split 200,000 generated arithmetic expressions into numbers, names and operators; print the count of each kind. |
| 38 | `calculator` | Evaluate 200,000 generated arithmetic expressions with + − × ÷ and parentheses by recursive descent; print the sum of the results. |
| 39 | `polynomial` | Multiply pairs of generated polynomials of degree 300 (coefficient lists) 2,000 times; print a checksum of the coefficients. |
| 40 | `intervals` | Merge 1,000,000 generated intervals after sorting them by start; print the number of merged intervals and their total length. |
