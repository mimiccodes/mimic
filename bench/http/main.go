// http: a hello-world server (net/http); bench/run.py measures it with load.go
package main

import "net/http"

func main() {
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		w.Write([]byte("Hello, World!"))
	})
	http.ListenAndServe("127.0.0.1:18432", nil)
}
