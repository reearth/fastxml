//! Async schema resolution example.
//!
//! Demonstrates how to use the async schema resolution API to parse XSD schemas
//! with import/include dependencies resolved asynchronously.
//!
//! Run with: cargo run --example async_schema_resolution --features tokio

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use fastxml::error::Result;
use fastxml::schema::Schema;
use fastxml::schema::fetcher::{AsyncSchemaFetcher, FetchResult};

/// A mock async fetcher that serves schemas from an in-memory map.
///
/// In production, you would use `AsyncDefaultFetcher` which combines
/// local file access with HTTP fetching via reqwest.
struct MockAsyncFetcher {
    schemas: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MockAsyncFetcher {
    fn new() -> Self {
        Self {
            schemas: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn add_schema(&self, url: &str, content: &str) {
        self.schemas
            .write()
            .insert(url.to_string(), content.as_bytes().to_vec());
    }
}

#[async_trait::async_trait]
impl AsyncSchemaFetcher for MockAsyncFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Simulate network delay
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let schemas = self.schemas.read();
        if let Some(content) = schemas.get(url) {
            println!("  [Fetcher] Fetched: {}", url);
            Ok(FetchResult {
                content: content.clone(),
                final_url: url.to_string(),
                redirected: false,
            })
        } else {
            Err(fastxml::schema::fetcher::error::FetchError::RequestFailed {
                url: url.to_string(),
                message: "Schema not found".to_string(),
            }
            .into())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Async Schema Resolution Example ===\n");

    // Example 1: Simple schema without imports
    example_simple_schema().await?;

    // Example 2: Schema with imports
    example_with_imports().await?;

    // Example 3: Nested imports (A -> B -> C)
    example_nested_imports().await?;

    // Example 4: Using the real AsyncDefaultFetcher
    #[cfg(feature = "tokio")]
    example_real_fetcher().await?;

    println!("All examples completed successfully!");
    Ok(())
}

/// Example 1: Parse a simple schema without any imports
async fn example_simple_schema() -> Result<()> {
    println!("--- Example 1: Simple Schema ---\n");

    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/simple">
    <xs:element name="greeting" type="xs:string"/>
    <xs:element name="count" type="xs:integer"/>
</xs:schema>"#;

    let fetcher = MockAsyncFetcher::new();

    let schema = Schema::builder()
        .add("http://example.com/simple.xsd", xsd.as_bytes())
        .resolve_with_async(&fetcher)
        .await?;

    println!("Parsed schema:");
    println!("  Target namespace: {:?}", schema.target_namespace);
    println!(
        "  Elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );
    println!("  Types count: {}", schema.types.len());
    println!();

    Ok(())
}

/// Example 2: Parse a schema that imports another schema
async fn example_with_imports() -> Result<()> {
    println!("--- Example 2: Schema with Imports ---\n");

    // The imported types schema
    let types_xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/types">
    <xs:simpleType name="EmailType">
        <xs:restriction base="xs:string">
            <xs:pattern value="[^@]+@[^@]+\.[^@]+"/>
        </xs:restriction>
    </xs:simpleType>
    <xs:simpleType name="PhoneType">
        <xs:restriction base="xs:string">
            <xs:pattern value="\d{3}-\d{4}-\d{4}"/>
        </xs:restriction>
    </xs:simpleType>
</xs:schema>"#;

    // The main schema that imports types
    let main_xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:t="http://example.com/types"
           targetNamespace="http://example.com/contact">
    <xs:import namespace="http://example.com/types" schemaLocation="types.xsd"/>
    <xs:element name="contact">
        <xs:complexType>
            <xs:sequence>
                <xs:element name="name" type="xs:string"/>
                <xs:element name="email" type="t:EmailType"/>
                <xs:element name="phone" type="t:PhoneType" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:element>
</xs:schema>"#;

    let fetcher = MockAsyncFetcher::new();
    fetcher.add_schema("http://example.com/types.xsd", types_xsd);

    println!("Resolving schema with imports...");
    let schema = Schema::builder()
        .add("http://example.com/contact.xsd", main_xsd.as_bytes())
        .resolve_with_async(&fetcher)
        .await?;

    println!("\nParsed schema:");
    println!("  Target namespace: {:?}", schema.target_namespace);
    println!(
        "  Elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );
    println!(
        "  Custom types: {:?}",
        schema
            .types
            .keys()
            .filter(|k| !k.starts_with("xs:") && !k.starts_with("gml:"))
            .collect::<Vec<_>>()
    );

    println!();

    Ok(())
}

/// Example 3: Parse schemas with nested imports (A imports B, B imports C)
async fn example_nested_imports() -> Result<()> {
    println!("--- Example 3: Nested Imports ---\n");

    // Base types (C)
    let base_xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/base">
    <xs:simpleType name="IDType">
        <xs:restriction base="xs:string">
            <xs:pattern value="[A-Z]{2}-[0-9]{6}"/>
        </xs:restriction>
    </xs:simpleType>
    <xs:simpleType name="TimestampType">
        <xs:restriction base="xs:dateTime"/>
    </xs:simpleType>
</xs:schema>"#;

    // Common types (B) - imports base
    let common_xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:base="http://example.com/base"
           targetNamespace="http://example.com/common">
    <xs:import namespace="http://example.com/base" schemaLocation="base.xsd"/>
    <xs:complexType name="AuditType">
        <xs:sequence>
            <xs:element name="createdAt" type="base:TimestampType"/>
            <xs:element name="createdBy" type="base:IDType"/>
        </xs:sequence>
    </xs:complexType>
</xs:schema>"#;

    // Main schema (A) - imports common
    let main_xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:common="http://example.com/common"
           xmlns:base="http://example.com/base"
           targetNamespace="http://example.com/order">
    <xs:import namespace="http://example.com/common" schemaLocation="common.xsd"/>
    <xs:element name="order">
        <xs:complexType>
            <xs:sequence>
                <xs:element name="orderId" type="base:IDType"/>
                <xs:element name="audit" type="common:AuditType"/>
                <xs:element name="items">
                    <xs:complexType>
                        <xs:sequence>
                            <xs:element name="item" maxOccurs="unbounded">
                                <xs:complexType>
                                    <xs:sequence>
                                        <xs:element name="productId" type="base:IDType"/>
                                        <xs:element name="quantity" type="xs:positiveInteger"/>
                                    </xs:sequence>
                                </xs:complexType>
                            </xs:element>
                        </xs:sequence>
                    </xs:complexType>
                </xs:element>
            </xs:sequence>
        </xs:complexType>
    </xs:element>
</xs:schema>"#;

    let fetcher = MockAsyncFetcher::new();
    fetcher.add_schema("http://example.com/base.xsd", base_xsd);
    fetcher.add_schema("http://example.com/common.xsd", common_xsd);

    println!("Resolving schema with nested imports...");
    println!("  order.xsd -> common.xsd -> base.xsd\n");

    let schema = Schema::builder()
        .add("http://example.com/order.xsd", main_xsd.as_bytes())
        .resolve_with_async(&fetcher)
        .await?;

    println!("\nParsed schema:");
    println!("  Target namespace: {:?}", schema.target_namespace);
    println!(
        "  Root elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );

    let custom_types: Vec<_> = schema
        .types
        .keys()
        .filter(|k| !k.starts_with("xs:") && !k.starts_with("gml:") && !k.contains(':'))
        .collect();
    println!("  Custom types: {:?}", custom_types);

    println!();

    Ok(())
}

/// Example 4: Using the real AsyncDefaultFetcher
///
/// This shows how to use the production-ready fetcher that combines
/// local file access with async HTTP fetching.
#[cfg(feature = "tokio")]
async fn example_real_fetcher() -> Result<()> {
    println!("--- Example 4: Real AsyncDefaultFetcher ---\n");

    // In a real application, you would use AsyncDefaultFetcher like this:
    //
    // use fastxml::schema::AsyncDefaultFetcher;
    //
    // let fetcher = AsyncDefaultFetcher::new()?;
    // // Or with a base directory for relative paths:
    // // let fetcher = AsyncDefaultFetcher::with_base_dir("/path/to/schemas")?;
    //
    // let schema = parse_xsd_with_imports_async(
    //     xsd_content.as_bytes(),
    //     "http://example.com/schema.xsd",
    //     &fetcher,
    // ).await?;
    //
    // This fetcher will:
    // 1. Try to read local files first (for file:// URLs or relative paths)
    // 2. Fall back to async HTTP requests using reqwest
    // 3. Follow redirects automatically

    println!("AsyncDefaultFetcher provides:");
    println!("  - Local file access (file:// URLs and relative paths)");
    println!("  - Async HTTP fetching via reqwest");
    println!("  - Automatic redirect following");
    println!();

    Ok(())
}
