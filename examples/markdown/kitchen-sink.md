# Kitchen Sink

A tour of standard Markdown features.

## Text Formatting

Normal paragraph with **bold**, *italic*, and `inline code`. You can also combine **_bold italic_**.

### Links and References

- [MDViewer on GitHub](https://github.com/larsakeekstrand/mdviewer)
- A bare URL: <https://example.com>

#### Note

> Blockquotes are great for callouts or pulled quotes.
> They can span multiple lines.

## Lists

Ordered list:

1. First item
2. Second item
3. Third item

Unordered list:

- Alpha
- Beta
  - Nested item
- Gamma

## Table

| Language | Paradigm    | Year |
|----------|-------------|------|
| Rust     | Systems     | 2010 |
| Python   | Multi       | 1991 |
| Go       | Concurrent  | 2009 |

## Code Block

```rust
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn main() {
    println!("{}", greet("world"));
}
```
