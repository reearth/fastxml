//! CollectMulti trait and implementations for multi-XPath collection.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::xpath::XPathSource;

use super::FallbackMode;
use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::functions::stream_for_each_impl;
use super::streaming;
use super::xpath_analyze::{self, NotStreamableReason, StreamableXPath, XPathAnalysis};

/// Trait for collecting multiple XPath results in a single pass.
///
/// This trait is implemented for tuples of `(&str, FnMut)` pairs,
/// allowing multiple XPath expressions to be evaluated in a single
/// streaming pass through the document.
pub trait CollectMulti<'a> {
    /// The output type (tuple of Vecs)
    type Output;

    /// Execute collection with the given input and namespaces.
    fn collect(
        self,
        input: &'a str,
        namespaces: &HashMap<String, String>,
        fallback_mode: FallbackMode,
    ) -> TransformResult<Self::Output>;
}

/// Macro to implement CollectMulti for tuples of various sizes
macro_rules! impl_collect_multi {
    // Base case for 2 elements
    (($($idx:tt: $T:ident, $F:ident);+)) => {
        impl<'a, $($T, $F),+> CollectMulti<'a> for ($((&'a str, $F),)+)
        where
            $($F: FnMut(&mut EditableNode) -> $T,)+
            $($T: 'static,)+
        {
            type Output = ($(Vec<$T>,)+);

            #[allow(non_snake_case)]
            fn collect(
                mut self,
                input: &'a str,
                namespaces: &HashMap<String, String>,
                fallback_mode: FallbackMode,
            ) -> TransformResult<Self::Output> {
                // Create result vectors wrapped in Rc<RefCell<>>
                $(
                    let $T: Rc<RefCell<Vec<$T>>> = Rc::new(RefCell::new(Vec::new()));
                )+

                // Parse and analyze all XPaths
                let mut xpaths = Vec::new();
                let mut streamable_xpaths = Vec::new();
                let mut all_streamable = true;
                let mut first_not_streamable: Option<(String, NotStreamableReason)> = None;

                $(
                    let xpath_str = self.$idx.0;
                    let xpath_source = XPathSource::String(xpath_str.to_string());
                    let expr = xpath_source.parse()?;
                    let analysis = xpath_analyze::analyze_xpath(&expr);
                    xpaths.push((xpath_str, xpath_source));

                    match analysis {
                        XPathAnalysis::Streamable(s) => {
                            streamable_xpaths.push(s);
                        }
                        XPathAnalysis::NotStreamable(reason) => {
                            all_streamable = false;
                            if first_not_streamable.is_none() {
                                first_not_streamable = Some((xpath_str.to_string(), reason));
                            }
                            // Push a dummy to keep indices aligned (will be handled in fallback)
                            streamable_xpaths.push(StreamableXPath::default());
                        }
                    }
                )+

                // If not all streamable, check fallback mode
                if !all_streamable {
                    match fallback_mode {
                        FallbackMode::Disabled => {
                            let (xpath, reason) = first_not_streamable.unwrap();
                            return Err(TransformError::NotStreamable { xpath, reason });
                        }
                        FallbackMode::Enabled => {
                            // Fall back to sequential processing (multiple passes)
                            $(
                                let result_clone = $T.clone();
                                let f = &mut self.$idx.1;
                                stream_for_each_impl(
                                    input,
                                    &xpaths[$idx].1,
                                    namespaces,
                                    fallback_mode,
                                    |node| {
                                        result_clone.borrow_mut().push(f(node));
                                    },
                                )?;
                            )+

                            return Ok(($(
                                $T.take(),
                            )+));
                        }
                    }
                }

                // All streamable - create callbacks that push to result vectors
                $(
                    let result_clone = $T.clone();
                    let func = &mut self.$idx.1;
                    #[allow(non_snake_case)]
                    let mut $F = move |node: &mut EditableNode| {
                        result_clone.borrow_mut().push(func(node));
                    };
                )+

                // Build handler pairs for process_for_each_multi
                let mut handlers: Vec<streaming::MultiHandler<'_>> = vec![
                    $((&streamable_xpaths[$idx], &mut $F as &mut dyn FnMut(&mut EditableNode)),)+
                ];

                streaming::process_for_each_multi(input, &mut handlers, namespaces)?;

                // Extract results from RefCell (take ownership of the Vec)
                Ok(($(
                    $T.take(),
                )+))
            }
        }
    };
}

// Implement for tuples of 2 to 8 elements
impl_collect_multi!((0: T0, F0; 1: T1, F1));
impl_collect_multi!((0: T0, F0; 1: T1, F1; 2: T2, F2));
impl_collect_multi!((0: T0, F0; 1: T1, F1; 2: T2, F2; 3: T3, F3));
impl_collect_multi!((0: T0, F0; 1: T1, F1; 2: T2, F2; 3: T3, F3; 4: T4, F4));
impl_collect_multi!((0: T0, F0; 1: T1, F1; 2: T2, F2; 3: T3, F3; 4: T4, F4; 5: T5, F5));
impl_collect_multi!((0: T0, F0; 1: T1, F1; 2: T2, F2; 3: T3, F3; 4: T4, F4; 5: T5, F5; 6: T6, F6));
impl_collect_multi!((0: T0, F0; 1: T1, F1; 2: T2, F2; 3: T3, F3; 4: T4, F4; 5: T5, F5; 6: T6, F6; 7: T7, F7));
