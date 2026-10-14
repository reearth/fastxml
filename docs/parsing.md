# Parsing and serializing

fastxml parses XML 1.0 into a DOM (`XmlDocument`) or into a stream of events,
and writes documents back out with `Printer`.

## DOM parsing

```rust
use fastxml::{Parser, QueryExt};

let xml = r#"<root><item id="1">Hello</item><item id="2">World</item></root>"#;

let doc = Parser::from(xml).parse()?;
for node in doc.query_nodes("//item")? {
    println!("{}: {}", node.get_attribute("id").unwrap(), node.get_content().unwrap());
}
```

`Parser::from` accepts `&str` or `&[u8]`; use `Parser::from_reader(reader)` to parse from any `BufRead`, and `.options(ParserOptions { .. })` to configure parsing.

## Serializing

`Printer` turns a parsed document or node back into XML:

```rust
use fastxml::{Parser, Printer};

let doc = Parser::from("<root><child>hi</child></root>").parse()?;

let xml = Printer::from(&doc).to_string()?;            // whole document, with <?xml ?>
let pretty = Printer::from(&doc).pretty().to_string()?; // indented

// Stream straight to any writer, no intermediate String:
Printer::from(&doc).write_to(&mut std::io::stdout())?;
```

`Printer::from` accepts `&XmlDocument`, `&XmlNode`, or `&XmlRoNode` (a document
emits an XML declaration by default, a single node does not). Builders:
`.pretty()` / `.indent(s)` / `.declaration(bool)` / `.encoding(s)`. Terminals:
`.to_string()` / `.into_bytes()` / `.write_to(w)`.

## Streaming events

For a quick, buffered list of events:

```rust
use fastxml::Parser;

for event in Parser::from(xml).events()? {
    // inspect each XmlEvent
}
```

To process large files with **constant memory**, use `for_each_event` — the callback is invoked as each event is read, nothing is buffered, and it may capture and mutate local state:

```rust
use fastxml::Parser;
use fastxml::event::XmlEvent;
use std::io::BufReader;
use std::fs::File;

let file = File::open("large_file.xml")?;

let mut elements = 0;
Parser::from_reader(BufReader::new(file)).for_each_event(|event| {
    if let XmlEvent::StartElement { .. } = event {
        elements += 1;
    }
    Ok(())
})?;
println!("{elements} elements");
```

## libxml compatibility

For migrating from libxml, the `fastxml::compat` module provides free functions
that mirror libxml's shape (`evaluate`, `create_context`, `get_root_node`,
`node_to_xml_string`, `find_nodes_by_xpath`, …). They are thin wrappers over the
modern front doors — prefer `Parser` / `Query` / `QueryExt` / `Printer` for new
code.

```rust
use fastxml::Parser;
use fastxml::compat::{evaluate, get_root_node};

let doc = Parser::from(xml).parse()?;
let root = get_root_node(&doc)?;          // modern: doc.get_root_element()
let items = evaluate(&doc, "//item")?;    // modern: doc.query("//item")
```

See [`examples/`](../examples/) (`query`, `printer`, `compat`, `dom_parsing`, …) for runnable
demonstrations of both the modern and compatibility APIs.

## Not supported

- XQuery, XSLT, XInclude
- DTD validation
- XML Signature/Encryption
- Reading XML catalogs (OASIS catalogs are not consulted when resolving
  schema locations; `fastxml::schema::export` only *writes* a `catalog.xml`
  for use by other tools)
- External entities (SYSTEM/PUBLIC) and parameter-entity expansion (internal
  general entities are expanded, including markup in their replacement text; a
  reference to an external entity is reported as an error)
