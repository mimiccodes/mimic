// JSON: encode 300,000 records to text, decode the text back into typed
// records, check them (serde + serde_json)
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Item {
    id: i64,
    name: String,
    score: f64,
    tags: Vec<String>,
}

fn main() {
    let items: Vec<Item> = (0..300000i64)
        .map(|i| Item { id: i, name: format!("item-{i}"), score: i as f64 * 0.5, tags: vec!["red".to_string(), format!("tag-{}", i % 100)] })
        .collect();
    let text = serde_json::to_string(&items).unwrap();
    let back: Vec<Item> = serde_json::from_str(&text).unwrap();
    let ids: i64 = back.iter().map(|it| it.id).sum();
    let chars: i64 = back.iter().map(|it| (it.name.len() + it.tags.iter().map(|t| t.len()).sum::<usize>()) as i64).sum();
    let good = back.iter().filter(|it| it.score == it.id as f64 * 0.5).count();
    println!("{} {} {} {}", back.len(), ids, chars, good);
}
