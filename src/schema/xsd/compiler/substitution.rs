//! Substitution group building and transitive closure computation.

use std::collections::HashSet;
use std::sync::Arc;

use crate::schema::types::CompiledSchema;

use super::XsdCompiler;

impl XsdCompiler {
    /// Builds the substitution group index.
    pub(crate) fn build_substitution_groups(&mut self, schema: &mut CompiledSchema) {
        // Collect substitution group relationships
        for elem in schema.elements_ns.values() {
            if let Some(sg_head) = &elem.substitution_group {
                self.substitution_groups
                    .entry(sg_head.clone())
                    .or_default()
                    .push(elem.name.clone());
            }
        }

        // Store in schema for validation use (schema map is FxHashMap; C6).
        schema.substitution_groups = self
            .substitution_groups
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }

    /// Builds the transitive substitution groups cache.
    ///
    /// This pre-computes all transitive members for each substitution group head,
    /// so validation doesn't need to recurse at runtime.
    pub(crate) fn build_transitive_substitution_groups(&self, schema: &mut CompiledSchema) {
        // Build reverse lookup (member -> head)
        // Register both prefixed and non-prefixed versions for efficient lookup
        for (head, members) in &schema.substitution_groups {
            for member in members {
                // Register with original name
                schema
                    .substitution_group_heads
                    .insert(member.clone(), head.clone());

                // Also register with local name (without prefix) for faster lookup
                if let Some((_prefix, local)) = member.split_once(':') {
                    schema
                        .substitution_group_heads
                        .entry(local.to_string())
                        .or_insert_with(|| head.clone());
                }
            }
        }

        // Build transitive closure for each head
        // Register both prefixed and non-prefixed versions for efficient lookup
        for head in schema.substitution_groups.keys() {
            let mut all_members = Vec::new();
            let mut visited = HashSet::new();
            self.collect_transitive_substitution_members(
                head,
                &schema.substitution_groups,
                &mut all_members,
                &mut visited,
            );
            let members_arc = Arc::new(all_members);

            // Register with original name
            schema
                .transitive_substitution_groups
                .insert(head.clone(), Arc::clone(&members_arc));

            // Also register with local name (without prefix) for faster lookup
            if let Some((_prefix, local)) = head.split_once(':') {
                schema
                    .transitive_substitution_groups
                    .entry(local.to_string())
                    .or_insert_with(|| Arc::clone(&members_arc));
            }
        }
    }

    /// Helper to recursively collect substitution group members.
    fn collect_transitive_substitution_members(
        &self,
        head_name: &str,
        groups: &rustc_hash::FxHashMap<String, Vec<String>>,
        members: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if visited.contains(head_name) {
            return;
        }
        visited.insert(head_name.to_string());

        // Try multiple name variants for namespace prefix handling
        let mut names_to_try = vec![head_name.to_string()];

        if let Some((_prefix, local)) = head_name.split_once(':') {
            names_to_try.push(local.to_string());
        } else {
            // No prefix -> try with common prefixes from schema
            for key in groups.keys() {
                if let Some((_prefix, local)) = key.split_once(':') {
                    if local == head_name {
                        names_to_try.push(key.clone());
                    }
                }
            }
        }

        for name in names_to_try {
            if let Some(direct_members) = groups.get(&name) {
                for member in direct_members {
                    if !members.contains(member) {
                        members.push(member.clone());
                    }
                    // Recursively collect this member's members
                    self.collect_transitive_substitution_members(member, groups, members, visited);
                }
            }
        }
    }
}
