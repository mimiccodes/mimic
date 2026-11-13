// Load generator for the http benchmark: `conns` keep-alive connections
// send `total` GET requests between them, `depth` at a time per connection
// (HTTP/1.1 pipelining), and check every response body. Prints the
// elapsed seconds.
//
//	load <port> <conns> <total> <depth> <expected body>
package main

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"
)

func main() {
	port, conns, total, depth, want := os.Args[1], atoi(os.Args[2]), atoi(os.Args[3]), atoi(os.Args[4]), os.Args[5]
	req := []byte("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
	batch := []byte(strings.Repeat(string(req), depth))
	var wg sync.WaitGroup
	errs := make(chan error, conns)
	start := time.Now()
	for c := 0; c < conns; c++ {
		n := total / conns
		if c < total%conns {
			n++
		}
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			conn, err := net.Dial("tcp", "127.0.0.1:"+port)
			if err != nil {
				errs <- err
				return
			}
			defer conn.Close()
			r := bufio.NewReader(conn)
			for i := 0; i < n; i += depth {
				k := depth
				if n-i < k {
					k = n - i
				}
				if _, err := conn.Write(batch[:k*len(req)]); err != nil {
					errs <- err
					return
				}
				for j := 0; j < k; j++ {
					body, err := response(r)
					if err != nil {
						errs <- err
						return
					}
					if body != want {
						errs <- fmt.Errorf("body %q, expected %q", body, want)
						return
					}
				}
			}
		}(n)
	}
	wg.Wait()
	elapsed := time.Since(start).Seconds()
	select {
	case err := <-errs:
		fmt.Fprintln(os.Stderr, "load:", err)
		os.Exit(1)
	default:
	}
	fmt.Printf("%.6f\n", elapsed)
}

// one response: status line, headers (Content-Length), body
func response(r *bufio.Reader) (string, error) {
	status, err := r.ReadString('\n')
	if err != nil {
		return "", err
	}
	if !strings.HasPrefix(status, "HTTP/1.1 200") {
		return "", fmt.Errorf("status %q", strings.TrimSpace(status))
	}
	length := -1
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			return "", err
		}
		line = strings.TrimRight(line, "\r\n")
		if line == "" {
			break
		}
		if k, v, ok := strings.Cut(line, ":"); ok && strings.EqualFold(k, "content-length") {
			length = atoi(strings.TrimSpace(v))
		}
	}
	if length < 0 {
		return "", fmt.Errorf("no Content-Length")
	}
	body := make([]byte, length)
	if _, err := io.ReadFull(r, body); err != nil {
		return "", err
	}
	return string(body), nil
}

func atoi(s string) int {
	n, err := strconv.Atoi(s)
	if err != nil {
		panic(err)
	}
	return n
}
