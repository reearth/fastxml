//! XPath with namespaces example.
//!
//! Demonstrates querying XML documents that use namespaces.
//!
//! Run with: cargo run --example xpath_namespaces

use fastxml::{evaluate, parse};

fn main() -> fastxml::error::Result<()> {
    // CityGML-style document with multiple namespaces
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                xmlns:gml="http://www.opengis.net/gml">
    <core:cityObjectMember>
        <bldg:Building gml:id="BLDG_001">
            <bldg:measuredHeight uom="m">25.5</bldg:measuredHeight>
            <bldg:function>1000</bldg:function>
            <bldg:yearOfConstruction>1995</bldg:yearOfConstruction>
        </bldg:Building>
    </core:cityObjectMember>
    <core:cityObjectMember>
        <bldg:Building gml:id="BLDG_002">
            <bldg:measuredHeight uom="m">32.0</bldg:measuredHeight>
            <bldg:function>2000</bldg:function>
            <bldg:yearOfConstruction>2005</bldg:yearOfConstruction>
        </bldg:Building>
    </core:cityObjectMember>
</core:CityModel>
"#;

    let doc = parse(xml.as_bytes())?;
    println!("=== XPath with Namespaces ===\n");

    // Query using namespace prefix
    println!("1. Find all buildings (//bldg:Building):");
    let result = evaluate(&doc, "//bldg:Building")?;
    for node in result.into_nodes() {
        let id = node
            .get_attribute("gml:id")
            .or_else(|| node.get_attribute("id"));
        println!("   Found: {}", id.unwrap_or_default());
    }

    // Query specific namespaced elements
    println!("\n2. Get all building heights (//bldg:measuredHeight/text()):");
    let result = evaluate(&doc, "//bldg:measuredHeight/text()")?;
    let heights = fastxml::xpath::collect_text_values(&result);
    for height in &heights {
        println!("   Height: {}m", height);
    }

    // Use local-name() function for dynamic matching
    println!("\n3. Find elements by local name (*[local-name()='Building']):");
    let result = evaluate(&doc, "//*[local-name()='Building']")?;
    println!("   Found {} buildings", result.into_nodes().len());

    // Query with predicates on namespaced attributes
    println!("\n4. Find building with specific ID (//bldg:Building[@gml:id='BLDG_002']):");
    let result = evaluate(&doc, "//bldg:Building[@gml:id='BLDG_002']")?;
    for node in result.into_nodes() {
        // Get child elements
        for child in node.get_child_elements() {
            if child.get_name().contains("measuredHeight") {
                println!(
                    "   BLDG_002 height: {}",
                    child.get_content().unwrap_or_default()
                );
            }
        }
    }

    // Query construction years
    println!("\n5. Get all construction years:");
    let result = evaluate(&doc, "//bldg:yearOfConstruction/text()")?;
    let years = fastxml::xpath::collect_text_values(&result);
    for year in &years {
        println!("   Year: {}", year);
    }

    // Query using namespace-uri() function
    println!("\n6. Find elements in building namespace:");
    let result = evaluate(
        &doc,
        "//*[namespace-uri()='http://www.opengis.net/citygml/building/2.0']",
    )?;
    let count = result.into_nodes().len();
    println!("   Found {} elements in building namespace", count);

    // Complex query: buildings built after 2000
    println!("\n7. Buildings with yearOfConstruction > 2000:");
    let result = evaluate(&doc, "//bldg:Building[bldg:yearOfConstruction > 2000]")?;
    for node in result.into_nodes() {
        let id = node.get_attribute("gml:id").unwrap_or_default();
        println!("   {}", id);
    }

    Ok(())
}
