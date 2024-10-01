// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Helper macro to register a workflow by name. Updated can select a registered workflow at run-time.
/// Currently, you must force linkage to the crate that contains the workflow function by declaring `extern crate <your workflow crate>;`
/// in updated's main.rs. Otherwise, the linker will skip the dependency as its symbols seem to be unused.
#[proc_macro_attribute]
pub fn workflow(args: TokenStream, input: TokenStream) -> TokenStream {
    // parse the annotated function
    let input = parse_macro_input!(input as ItemFn);
    let is_exiting = args.to_string() == "exiting";

    let ItemFn {
        attrs,
        vis,
        sig,
        block,
    } = input;

    // get the name of the function
    let function_identifier = sig.ident.clone();

    if is_exiting {
        quote! (
            // register the function with the inventory crate
            libupdated::inventory::submit! {
                libupdated::update_workflow::WorkflowPlugin::new(stringify!(#function_identifier), |config| -> libupdated::BoxFuture<'static, Result<std::process::ExitCode, WorkflowError>> {
                    Box::pin(#function_identifier(config))
                })
            }

            // output the original function unchanged
            #(#attrs)* #vis #sig #block
        ).into()
    } else {
        // if the workflow is not exiting, we wrap it in a function that returns an ExitCode
        quote! (
            // register the function with the inventory crate
            libupdated::inventory::submit! {
                libupdated::update_workflow::WorkflowPlugin::new(stringify!(#function_identifier), |config| -> libupdated::BoxFuture<'static, Result<std::process::ExitCode, WorkflowError>> {
                    async fn wrapper_function(config: HashMap<String, String>) -> Result<std::process::ExitCode, WorkflowError> {
                        #function_identifier(config).await?;
                        Ok(std::process::ExitCode::SUCCESS)
                    }

                    Box::pin(wrapper_function(config))
                })
            }

            // output the original function unchanged
            #(#attrs)* #vis #sig #block
        ).into()
    }
}
