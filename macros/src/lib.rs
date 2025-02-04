use ignore::WalkBuilder;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use regex::Regex;
use std::path::{MAIN_SEPARATOR, Path};
use syn::{Attribute, ImplItem, ItemFn, ItemImpl, LitStr, parse_macro_input};

#[proc_macro_attribute]
pub fn gen_tests_rs(input_path: TokenStream, input_fn: TokenStream) -> TokenStream {
    let re = Regex::new(r"folder=([^,]+),ignore_files=\[([^)]+)\]").unwrap();
    let binding = input_path.to_string();
    let caps = re.captures(binding.as_str()).unwrap();

    //caps[1] = folder name
    //caps[2] = ignore file names
    let mut tests = proc_macro2::TokenStream::new();
    let test_fn = parse_macro_input!(input_fn as ItemFn);
    let test_name_fn = &test_fn.sig.ident;

    let mut binding = WalkBuilder::new(Path::new(&caps[1]));

    let mut file_walker = binding.hidden(false).ignore(false);
    file_walker = caps[2].split(',').fold(file_walker, |walker, file| {
        walker.add_custom_ignore_filename(file.trim())
    });

    let file_walker = file_walker.build().filter_map(|entry| entry.ok());

    for entry in file_walker {
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }

        let test_name = format_ident!("{}", get_test_name(path));
        let test_path = &path.display().to_string();

        tests.extend(quote! {
            #[test]
            fn #test_name() {
                #test_name_fn(#test_path)
            }
        });
    }

    TokenStream::from(quote! {
        mod #test_name_fn {
            use super::*;
            #test_fn
            #tests
        }
    })
}

#[proc_macro_attribute]
pub fn gen_tests_toml(input_path: TokenStream, input_fn: TokenStream) -> TokenStream {
    let mut tests = proc_macro2::TokenStream::new();
    let test_fn = parse_macro_input!(input_fn as ItemFn);
    let test_name_fn = &test_fn.sig.ident;

    let ignore_filename = format!(".{}.ignore", test_name_fn);
    for entry in WalkBuilder::new(Path::new(&parse_macro_input!(input_path as LitStr).value()))
        .hidden(false)
        .ignore(false)
        .add_custom_ignore_filename(ignore_filename.clone())
        .build()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if !path.join("Cargo.toml").exists() {
            continue;
        }

        let test_name = format_ident!("{}", get_test_name(path));
        let test_path = &path.display().to_string();
        tests.extend(quote! {
            #[test]
            fn #test_name() {
                #test_name_fn(#test_path)
            }
        });
    }

    TokenStream::from(quote! {
        mod #test_name_fn {
            use super::*;
            #test_fn
            #tests
        }
    })
}

fn get_test_name(path: &Path) -> String {
    path.with_extension("")
        .to_string_lossy()
        .replace("-", "_")
        .replace(MAIN_SEPARATOR, "__")
}

fn function_contains_log_fn(st: &str) -> bool {
    st.contains("tracing")
}

#[proc_macro_attribute]
pub fn trait_log_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr: proc_macro2::TokenStream = attr.into();
    let mut input = parse_macro_input!(item as ItemImpl);
    let instrument_stmt: Attribute = syn::parse_quote! {
        #[tracing::instrument(#attr)]
    };
    let clippy_ignore: Attribute = syn::parse_quote! {
    #[cfg_attr(any(feature = "profile_flame", feature = "profile_tracy", feature = "profile_full"),
               clippy_tracing_attributes::clippy_tracing_skip)]
        };

    for item in &mut input.items {
        if let ImplItem::Fn(method) = item {
            let att = &mut method.attrs;
            let att_string = format!("{:?}", att);

            if !function_contains_log_fn(att_string.as_str()) {
                att.insert(0, instrument_stmt.clone());
                att.insert(0, clippy_ignore.clone());
            }
        }
    }
    let output = quote! {
        #input
    };
    output.into()
}
