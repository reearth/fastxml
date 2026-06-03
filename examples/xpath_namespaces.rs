//! XPath with namespaces example.
//!
//! Demonstrates querying namespaced documents with the modern `QueryExt`
//! (`doc.query(..)`) API.
//!
//! Run with: cargo run --example xpath_namespaces

use fastxml::{Parser, QueryExt};

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

    let doc = Parser::from(xml.as_bytes()).parse()?;
    println!("=== XPath with Namespaces ===\n");

    // Query using a namespace prefix. Prefixes declared on the root element are
    // registered automatically.
    println!("1. Find all buildings (//bldg:Building):");
    for node in doc.query_nodes("//bldg:Building")? {
        // Attributes are stored with local names (libxml compatible)
        let id = node.get_attribute("id");
        println!("   Found: {}", id.unwrap_or_default());
    }

    // Query specific namespaced elements.
    println!("\n2. Get all building heights (//bldg:measuredHeight/text()):");
    let heights = doc.query("//bldg:measuredHeight/text()")?;
    for height in fastxml::xpath::collect_text_values(&heights) {
        println!("   Height: {}m", height);
    }

    // Use local-name() for dynamic matching.
    println!("\n3. Find elements by local name (*[local-name()='Building']):");
    let buildings = doc.query_nodes("//*[local-name()='Building']")?;
    println!("   Found {} buildings", buildings.len());

    // Query with predicates on namespaced attributes.
    println!("\n4. Find building with specific ID (//bldg:Building[@gml:id='BLDG_002']):");
    for node in doc.query_nodes("//bldg:Building[@gml:id='BLDG_002']")? {
        for child in node.get_child_elements() {
            if child.get_name().contains("measuredHeight") {
                println!(
                    "   BLDG_002 height: {}",
                    child.get_content().unwrap_or_default()
                );
            }
        }
    }

    // Query construction years.
    println!("\n5. Get all construction years:");
    let years = doc.query("//bldg:yearOfConstruction/text()")?;
    for year in fastxml::xpath::collect_text_values(&years) {
        println!("   Year: {}", year);
    }

    // Query using namespace-uri().
    println!("\n6. Find elements in building namespace:");
    let in_ns =
        doc.query_nodes("//*[namespace-uri()='http://www.opengis.net/citygml/building/2.0']")?;
    println!("   Found {} elements in building namespace", in_ns.len());

    // Complex query: buildings built after 2000.
    println!("\n7. Buildings with yearOfConstruction > 2000:");
    for node in doc.query_nodes("//bldg:Building[bldg:yearOfConstruction > 2000]")? {
        let id = node.get_attribute("id").unwrap_or_default();
        println!("   {}", id);
    }

    Ok(())
}
