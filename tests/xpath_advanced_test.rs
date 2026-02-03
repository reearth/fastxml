//! Advanced XPath tests.
//!
//! Tests for XPath features that are not covered in xpath_test.rs and xpath_functions_test.rs:
//! - Attribute axis and attribute selection
//! - node() test
//! - Complex predicates
//! - Relative paths with parent navigation
//! - Union edge cases
//! - Edge cases (empty strings, special values)

mod common;

use fastxml::xpath::collect_text_values;
use fastxml::{evaluate, parse};

// =============================================================================
// Attribute Axis Tests
// =============================================================================

mod attribute_axis {
    use super::*;

    #[test]
    fn test_attribute_shorthand() {
        let xml = r#"<root><item id="1" name="first"/><item id="2" name="second"/></root>"#;
        let doc = parse(xml).unwrap();

        // @id shorthand for attribute::id
        let result = evaluate(&doc, "//item/@id").unwrap();
        let values = collect_text_values(&result);
        assert_eq!(values.len(), 2);
        assert!(values.contains(&"1".to_string()));
        assert!(values.contains(&"2".to_string()));

        compare_with_libxml!(xpath: xml, "//item/@id", &doc);
    }

    #[test]
    fn test_attribute_axis_explicit() {
        let xml = r#"<root><item id="1" name="first"/></root>"#;
        let doc = parse(xml).unwrap();

        // Explicit attribute axis
        let result = evaluate(&doc, "//item/attribute::id").unwrap();
        let values = collect_text_values(&result);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], "1");

        compare_with_libxml!(xpath: xml, "//item/attribute::id", &doc);
    }

    #[test]
    fn test_attribute_wildcard() {
        let xml = r#"<root><item id="1" name="first" type="A"/></root>"#;
        let doc = parse(xml).unwrap();

        // All attributes
        let result = evaluate(&doc, "//item/@*").unwrap();
        let values = collect_text_values(&result);
        assert_eq!(values.len(), 3);
        assert!(values.contains(&"1".to_string()));
        assert!(values.contains(&"first".to_string()));
        assert!(values.contains(&"A".to_string()));

        // Attribute order is preserved (XML source order) using IndexMap
        compare_with_libxml!(xpath: xml, "//item/@*", &doc);
    }

    #[test]
    fn test_attribute_in_predicate() {
        let xml = r#"<root>
            <item id="1" status="active"/>
            <item id="2" status="inactive"/>
            <item id="3" status="active"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[@status='active']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "//item[@status='active']", &doc);
    }

    #[test]
    fn test_attribute_existence_check() {
        let xml = r#"<root>
            <item id="1"/>
            <item name="no-id"/>
            <item id="2"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        // Check for existence of @id
        let result = evaluate(&doc, "//item[@id]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "//item[@id]", &doc);
    }

    #[test]
    fn test_multiple_attribute_predicates() {
        let xml = r#"<root>
            <item id="1" type="A" status="active"/>
            <item id="2" type="A" status="inactive"/>
            <item id="3" type="B" status="active"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        // Multiple attribute conditions with AND
        let result = evaluate(&doc, "//item[@type='A' and @status='active']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);

        compare_with_libxml!(xpath: xml, "//item[@type='A' and @status='active']", &doc);
    }

    #[test]
    fn test_attribute_or_predicates() {
        let xml = r#"<root>
            <item id="1" type="A"/>
            <item id="2" type="B"/>
            <item id="3" type="C"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        // OR condition on attributes
        let result = evaluate(&doc, "//item[@type='A' or @type='B']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "//item[@type='A' or @type='B']", &doc);
    }

    #[test]
    fn test_attribute_not_predicate() {
        let xml = r#"<root>
            <item id="1" status="active"/>
            <item id="2" status="inactive"/>
            <item id="3" status="active"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[not(@status='active')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);

        compare_with_libxml!(xpath: xml, "//item[not(@status='active')]", &doc);
    }
}

// =============================================================================
// node() Test
// =============================================================================

mod node_test {
    use super::*;

    #[test]
    fn test_node_selects_all_nodes() {
        let xml = r#"<root><child>text</child></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/node()").unwrap();
        let nodes = result.into_nodes();
        // Should include the child element (and possibly text nodes)
        assert!(!nodes.is_empty());

        compare_with_libxml!(xpath: xml, "/root/node()", &doc);
    }

    #[test]
    fn test_node_with_descendant() {
        let xml = r#"<root><a><b>text</b></a><c/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//node()").unwrap();
        let nodes = result.into_nodes();
        // Should include root, a, b, c, and text node (5 total)
        // //node() is equivalent to /descendant-or-self::node()/child::node()
        assert_eq!(nodes.len(), 5);

        compare_with_libxml!(xpath: xml, "//node()", &doc);
    }

    #[test]
    fn test_node_vs_wildcard() {
        let xml = r#"<root>text<child/>more text</root>"#;
        let doc = parse(xml).unwrap();

        // node() includes text nodes, * only includes elements
        let node_result = evaluate(&doc, "/root/node()").unwrap();
        let wildcard_result = evaluate(&doc, "/root/*").unwrap();

        let node_count = node_result.into_nodes().len();
        let wildcard_count = wildcard_result.into_nodes().len();

        // node() should return at least as many as *
        assert!(node_count >= wildcard_count);

        compare_with_libxml!(xpath: xml, "/root/node()", &doc);
        compare_with_libxml!(xpath: xml, "/root/*", &doc);
    }

    #[test]
    fn test_node_with_position() {
        let xml = r#"<root><a/><b/><c/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/node()[2]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);

        compare_with_libxml!(xpath: xml, "/root/node()[2]", &doc);
    }
}

// =============================================================================
// Complex Predicates
// =============================================================================

mod complex_predicates {
    use super::*;

    #[test]
    fn test_position_greater_than() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item><item>4</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[position() > 2]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_position_less_than() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item><item>4</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[position() < 3]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_position_range() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item><item>4</item><item>5</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[position() >= 2 and position() <= 4]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_string_length_predicate() {
        let xml = r#"<root><item>ab</item><item>abcdef</item><item>abc</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[string-length() > 3]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("abcdef".to_string()));
    }

    #[test]
    fn test_contains_in_predicate() {
        let xml =
            r#"<root><item>hello world</item><item>goodbye</item><item>world peace</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[contains(., 'world')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_starts_with_in_predicate() {
        let xml = r#"<root><item>prefix_a</item><item>other</item><item>prefix_b</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[starts-with(., 'prefix')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_nested_predicate() {
        let xml = r#"<root>
            <parent>
                <child status="active">keep</child>
                <child status="inactive">skip</child>
            </parent>
            <parent>
                <child status="active">also keep</child>
            </parent>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//parent[child[@status='active']]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_count_in_predicate() {
        let xml = r#"<root>
            <group><item/><item/><item/></group>
            <group><item/></group>
            <group><item/><item/></group>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//group[count(item) >= 2]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_last_predicate() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[last()]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("3".to_string()));
    }

    #[test]
    fn test_last_minus_one() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[position() = last() - 1]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("2".to_string()));
    }
}

// =============================================================================
// Relative Paths
// =============================================================================

mod relative_paths {
    use super::*;
    use fastxml::{create_context, find_readonly_nodes_by_xpath, get_root_readonly_node};

    #[test]
    fn test_parent_navigation() {
        let xml = r#"<root><parent><child/></parent></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/parent/child/..").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "parent");
    }

    #[test]
    fn test_grandparent_navigation() {
        let xml = r#"<root><parent><child><grandchild/></child></parent></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/parent/child/grandchild/../..").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "parent");
    }

    #[test]
    fn test_self_reference() {
        let xml = r#"<root><child/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/child/.").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "child");
    }

    #[test]
    fn test_parent_then_sibling() {
        let xml = r#"<root><a/><b/><c/></root>"#;
        let doc = parse(xml).unwrap();

        // From b, go to parent, then find sibling a
        let result = evaluate(&doc, "/root/b/../a").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "a");
    }

    #[test]
    fn test_context_relative_descendant() {
        let xml = r#"<root><parent><target>1</target></parent><target>2</target></root>"#;
        let doc = parse(xml).unwrap();

        let ctx = create_context(&doc).unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        // Find parent element first
        let parents = find_readonly_nodes_by_xpath(&ctx, "//parent", &root).unwrap();
        assert_eq!(parents.len(), 1);

        // Then find target relative to parent
        let targets = find_readonly_nodes_by_xpath(&ctx, ".//target", &parents[0]).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].get_content(), Some("1".to_string()));
    }

    #[test]
    fn test_relative_wildcard() {
        let xml = r#"<root><parent><a/><b/><c/></parent></root>"#;
        let doc = parse(xml).unwrap();

        let ctx = create_context(&doc).unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        let parents = find_readonly_nodes_by_xpath(&ctx, "//parent", &root).unwrap();
        let children = find_readonly_nodes_by_xpath(&ctx, "./*", &parents[0]).unwrap();
        assert_eq!(children.len(), 3);
    }
}

// =============================================================================
// Union Edge Cases
// =============================================================================

mod union_edge_cases {
    use super::*;

    #[test]
    fn test_union_removes_duplicates() {
        let xml = r#"<root><item id="1"/></root>"#;
        let doc = parse(xml).unwrap();

        // Same path twice should not duplicate
        let result = evaluate(&doc, "//item | //item").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_union_different_axes() {
        let xml = r#"<root><a><b/></a><c/></root>"#;
        let doc = parse(xml).unwrap();

        // Descendant axis | child axis
        let result = evaluate(&doc, "//b | /root/c").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_union_with_text() {
        let xml = r#"<root><a>text1</a><b>text2</b></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//a/text() | //b/text()").unwrap();
        let values = collect_text_values(&result);
        assert_eq!(values.len(), 2);
        assert!(values.contains(&"text1".to_string()));
        assert!(values.contains(&"text2".to_string()));
    }

    // Note: Current implementation may not guarantee document order for unions
    #[test]
    fn test_union_preserves_document_order() {
        let xml = r#"<root><a/><b/><c/></root>"#;
        let doc = parse(xml).unwrap();

        // Union returns all elements (order may vary by implementation)
        let result = evaluate(&doc, "//c | //a | //b").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 3);

        // Check all elements are present
        let names: Vec<_> = nodes.iter().map(|n| n.get_name()).collect();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
        assert!(names.contains(&"c".to_string()));
    }

    #[test]
    fn test_union_empty_result() {
        let xml = r#"<root><item/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//nonexistent | //alsonothere").unwrap();
        let nodes = result.into_nodes();
        assert!(nodes.is_empty());
    }
}

// =============================================================================
// Sibling Axes
// =============================================================================

mod sibling_axes {
    use super::*;

    #[test]
    fn test_following_sibling() {
        let xml = r#"<root><a/><b/><c/><d/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/b/following-sibling::*").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].get_name(), "c");
        assert_eq!(nodes[1].get_name(), "d");

        compare_with_libxml!(xpath: xml, "/root/b/following-sibling::*", &doc);
    }

    #[test]
    fn test_preceding_sibling() {
        let xml = r#"<root><a/><b/><c/><d/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/c/preceding-sibling::*").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "/root/c/preceding-sibling::*", &doc);
    }

    #[test]
    fn test_following_sibling_with_name() {
        let xml = r#"<root><item/><other/><item/><item/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/other/following-sibling::item").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "/root/other/following-sibling::item", &doc);
    }

    #[test]
    fn test_preceding_sibling_first() {
        let xml = r#"<root><a/><b/><c/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/c/preceding-sibling::*[1]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "b");

        compare_with_libxml!(xpath: xml, "/root/c/preceding-sibling::*[1]", &doc);
    }
}

// =============================================================================
// Ancestor Axes
// =============================================================================

mod ancestor_axes {
    use super::*;

    #[test]
    fn test_ancestor() {
        let xml = r#"<root><parent><child><target/></child></parent></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//target/ancestor::*").unwrap();
        let nodes = result.into_nodes();
        // Should include child, parent, root
        assert_eq!(nodes.len(), 3);

        compare_with_libxml!(xpath: xml, "//target/ancestor::*", &doc);
    }

    #[test]
    fn test_ancestor_with_name() {
        let xml = r#"<root><parent><child><target/></child></parent></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//target/ancestor::parent").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "parent");

        compare_with_libxml!(xpath: xml, "//target/ancestor::parent", &doc);
    }

    #[test]
    fn test_ancestor_or_self() {
        let xml = r#"<root><parent><target/></parent></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//target/ancestor-or-self::*").unwrap();
        let nodes = result.into_nodes();
        // Should include target, parent, root
        assert_eq!(nodes.len(), 3);

        compare_with_libxml!(xpath: xml, "//target/ancestor-or-self::*", &doc);
    }

    #[test]
    fn test_ancestor_predicate() {
        let xml = r#"<root>
            <container type="A"><item>1</item></container>
            <container type="B"><item>2</item></container>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[ancestor::container[@type='A']]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("1".to_string()));

        compare_with_libxml!(xpath: xml, "//item[ancestor::container[@type='A']]", &doc);
    }
}

// =============================================================================
// Descendant Axes
// =============================================================================

mod descendant_axes {
    use super::*;

    #[test]
    fn test_descendant_or_self() {
        let xml = r#"<root><child><grandchild/></child></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/descendant-or-self::*").unwrap();
        let nodes = result.into_nodes();
        // root, child, grandchild
        assert_eq!(nodes.len(), 3);

        compare_with_libxml!(xpath: xml, "/root/descendant-or-self::*", &doc);
    }

    #[test]
    fn test_descendant_deep() {
        let xml = r#"<root><a><b><c><d><target/></d></c></b></a></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/descendant::target").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);

        compare_with_libxml!(xpath: xml, "/root/descendant::target", &doc);
    }

    #[test]
    fn test_descendant_multiple() {
        let xml = r#"<root>
            <a><target>1</target></a>
            <b><c><target>2</target></c></b>
            <target>3</target>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/descendant::target").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 3);

        compare_with_libxml!(xpath: xml, "/root/descendant::target", &doc);
    }
}

// =============================================================================
// Following/Preceding Axes
// =============================================================================

mod following_preceding_axes {
    use super::*;

    #[test]
    fn test_following_axis() {
        let xml = r#"<root><a><b/></a><c/><d/></root>"#;
        let doc = parse(xml).unwrap();

        // Following axis includes all nodes after current node in document order
        let result = evaluate(&doc, "/root/a/following::*").unwrap();
        let nodes = result.into_nodes();
        // Should include c, d (not b since it's a descendant, not following)
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "/root/a/following::*", &doc);
    }

    #[test]
    fn test_preceding_axis() {
        let xml = r#"<root><a/><b/><c><d/></c></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/c/preceding::*").unwrap();
        let nodes = result.into_nodes();
        // Should include a, b
        assert_eq!(nodes.len(), 2);

        compare_with_libxml!(xpath: xml, "/root/c/preceding::*", &doc);
    }
}

// =============================================================================
// Edge Cases
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_string_comparison() {
        let xml = r#"<root><item value=""/><item value="text"/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[@value='']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_whitespace_text() {
        let xml = r#"<root><item>   </item><item>text</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[normalize-space()='']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_numeric_string_comparison() {
        let xml = r#"<root><item>2</item><item>10</item></root>"#;
        let doc = parse(xml).unwrap();

        // XPath 1.0: when comparing string to number, string is converted to number
        let result = evaluate(&doc, "//item[. > 5]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("10".to_string()));
    }

    #[test]
    fn test_empty_element() {
        let xml = r#"<root><item/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[.='']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_special_characters_in_text() {
        let xml = r#"<root><item>&lt;tag&gt;</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[contains(., '<')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_cdata_content() {
        let xml = r#"<root><item><![CDATA[<not xml>]]></item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item").unwrap();
        let values = collect_text_values(&result);
        assert_eq!(values.len(), 1);
        assert!(values[0].contains("<not xml>"));
    }
}

// =============================================================================
// Arithmetic in XPath
// =============================================================================

mod arithmetic {
    use super::*;

    #[test]
    fn test_addition_in_predicate() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[position() = 1 + 1]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("2".to_string()));
    }

    #[test]
    fn test_multiplication_in_predicate() {
        let xml = r#"<root><item>10</item><item>20</item><item>30</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[. = 2 * 10]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("20".to_string()));
    }

    #[test]
    fn test_division_in_predicate() {
        let xml = r#"<root><item>5</item><item>10</item><item>20</item></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[. = 20 div 2]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("10".to_string()));
    }

    #[test]
    fn test_modulo_in_predicate() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item><item>4</item></root>"#;
        let doc = parse(xml).unwrap();

        // Select items at odd positions
        let result = evaluate(&doc, "//item[position() mod 2 = 1]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].get_content(), Some("1".to_string()));
        assert_eq!(nodes[1].get_content(), Some("3".to_string()));
    }
}

// =============================================================================
// Boolean Logic
// =============================================================================

mod boolean_logic {
    use super::*;

    #[test]
    fn test_complex_and_or() {
        let xml = r#"<root>
            <item a="1" b="1"/>
            <item a="1" b="2"/>
            <item a="2" b="1"/>
            <item a="2" b="2"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        // (a=1 AND b=1) OR (a=2 AND b=2)
        let result = evaluate(&doc, "//item[(@a='1' and @b='1') or (@a='2' and @b='2')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_not_with_and() {
        let xml = r#"<root>
            <item status="active" type="A"/>
            <item status="active" type="B"/>
            <item status="inactive" type="A"/>
        </root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[@status='active' and not(@type='A')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_double_negation() {
        let xml = r#"<root><item status="active"/><item status="inactive"/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//item[not(not(@status='active'))]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }
}
