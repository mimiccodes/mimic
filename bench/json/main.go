// JSON: encode 300,000 records to text, decode the text back into typed
// records, check them
package main

import (
	"encoding/json"
	"fmt"
)

type Item struct {
	ID    int64    `json:"id"`
	Name  string   `json:"name"`
	Score float64  `json:"score"`
	Tags  []string `json:"tags"`
}

func main() {
	items := make([]Item, 300000)
	for i := range items {
		items[i] = Item{int64(i), fmt.Sprintf("item-%d", i), float64(i) * 0.5, []string{"red", fmt.Sprintf("tag-%d", i%100)}}
	}
	text, err := json.Marshal(items)
	if err != nil {
		panic(err)
	}
	var back []Item
	if err := json.Unmarshal(text, &back); err != nil {
		panic(err)
	}
	var ids, chars, good int64
	for _, it := range back {
		ids += it.ID
		chars += int64(len(it.Name))
		for _, t := range it.Tags {
			chars += int64(len(t))
		}
		if it.Score == float64(it.ID)*0.5 {
			good++
		}
	}
	fmt.Printf("%d %d %d %d\n", len(back), ids, chars, good)
}
