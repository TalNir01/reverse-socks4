# tagger

A Rust library for tagging and handling data with associated metadata.

## Features
- Tag arbitrary data with custom metadata
- Serialize and deserialize tagged data
- Utilities for working with tagged data structures

## Usage
Add to your `Cargo.toml`:

```toml
[dependencies]
tagger = { path = "../tagger" }
```

Import and use in your Rust code:

```rust
use tagger::{TaggedData, Tag};

let tag = generate_client_id;
let data = vec![1, 2, 3];
let tagged = TaggedData::new(tag, data);
println!("{:?}", tagged);
```

## Documentation
- See the source code and inline docs for API details.

## License
MIT
