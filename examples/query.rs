//! XPath querying with the modern `Query` / `QueryExt` / `StreamableQuery` APIs.
//!
//! Run with: cargo run --example query

use fastxml::transform::{StreamableQuery, Transformer};
use fastxml::{Parser, Query, QueryExt};

// Transform operations return `TransformError`, which converts into the
// crate-wide `fastxml::error::Error`, so a single `Result` type works throughout.
fn main() -> fastxml::error::Result<()> {
    println!("=== Querying with Query / QueryExt ===\n");

    // 1. QueryExt: method-call ergonomics on a document for one-off lookups.
    let doc = Parser::from("<root><item id='1'/><item id='2'/></root>").parse()?;
    println!(
        "1. doc.query_nodes(\"//item\") -> {}",
        doc.query_nodes("//item")?.len()
    );
    println!(
        "   doc.query(\"count(//item)\") -> {}",
        doc.query("count(//item)")?.to_number()
    );

    // 2. Query::compile once, evaluate against many documents (no re-parse).
    let query = Query::compile("//item")?;
    let a = Parser::from("<root><item/><item/><item/></root>").parse()?;
    let b = Parser::from("<root><item/></root>").parse()?;
    println!("\n2. Compiled query reused across documents:");
    println!("   doc a -> {} items", query.find_nodes(&a)?.len());
    println!("   doc b -> {} items", query.find_nodes(&b)?.len());

    // A compiled query renders back to an equivalent XPath string.
    println!("   query.to_string() -> {:?}", query.to_string());

    // 3. Namespaces: prefixes on the root are auto-registered; add extras with
    //    `.namespace(..)`.
    let ns_doc = Parser::from(
        r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point/><gml:point/></root>"#,
    )
    .parse()?;
    let pts = Query::compile("//gml:point")?.find_nodes(&ns_doc)?;
    println!("\n3. Namespaced query //gml:point -> {} points", pts.len());

    // 4. eval_from: evaluate relative to a context node.
    let grouped = Parser::from("<root><group><item/><item/></group></root>").parse()?;
    let group = grouped.query_nodes("//group")?.remove(0);
    let rel = Query::compile("item")?.eval_from(&grouped, &group)?;
    println!(
        "\n4. eval_from(group, \"item\") -> {} items",
        rel.into_nodes().len()
    );

    // 5. StreamableQuery: the transform-side counterpart. Compiling validates
    //    streamability up front.
    println!("\n=== StreamableQuery (for Transformer) ===\n");
    let sq = StreamableQuery::compile("//item")?;
    println!("5. StreamableQuery::compile(\"//item\") -> ok");
    match StreamableQuery::compile("//item[last()]") {
        Ok(_) => println!("   (unexpected)"),
        Err(e) => println!("   StreamableQuery::compile(\"//item[last()]\") -> rejected: {e}"),
    }

    let out = Transformer::from("<root><item/><item/></root>")
        .on(&sq, |node| node.set_attribute("seen", "1"))
        .to_string()?;
    println!("   transformed: {out}");

    // A streamable query is a subset of a full query, so it also evaluates.
    println!(
        "   used as a Query too: {} nodes",
        ns_doc.query_nodes("//gml:point")?.len()
    );

    Ok(())
}
