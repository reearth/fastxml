//! Serializing XML with the `Printer` front door.
//!
//! Run with: cargo run --example printer

use std::io::Write;

use fastxml::{Parser, Printer, QueryExt};

fn main() -> fastxml::error::Result<()> {
    let doc = Parser::from(
        r#"<catalog><book id="1"><title>XML</title></book><book id="2"><title>Rust</title></book></catalog>"#,
    )
    .parse()?;

    println!("=== Printer ===\n");

    // 1. Whole document. A document emits an XML declaration by default.
    println!("1. Whole document:");
    println!("{}\n", Printer::from(&doc).to_string()?);

    // 2. Pretty-printed (indented). Turn the declaration off for brevity.
    println!("2. Pretty-printed, no declaration:");
    println!(
        "{}\n",
        Printer::from(&doc)
            .declaration(false)
            .pretty()
            .to_string()?
    );

    // 3. A single node. A node emits no declaration by default.
    let first_book = doc.query_nodes("//book")?.remove(0);
    println!("3. A single node:");
    println!("{}\n", Printer::from(&first_book).to_string()?);

    // 4. Custom indent string + bytes.
    let bytes = Printer::from(&doc)
        .declaration(false)
        .indent("    ")
        .into_bytes()?;
    println!("4. into_bytes() produced {} bytes", bytes.len());

    // 5. Stream straight to a writer (no intermediate String).
    print!("5. write_to(stdout): ");
    Printer::from(&first_book).write_to(&mut std::io::stdout())?;
    std::io::stdout().flush().ok();
    println!();

    Ok(())
}
