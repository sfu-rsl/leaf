use std::env;
use std::path::{MAIN_SEPARATOR, Path};

use ignore::WalkBuilder;
use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Ident, ImplItem, ItemFn, ItemImpl, LitStr, Token, parse_macro_input};

#[proc_macro_attribute]
pub fn gen_sample_tests_rs(input_args: TokenStream, input_fn: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input_args as TestArgs);
    let test_fn = parse_macro_input!(input_fn as ItemFn);
    match expand_sample_tests(args, test_fn) {
        Ok(modules) => TokenStream::from(modules),
        Err(error) => TokenStream::from(error.into_compile_error()),
    }
}

struct TestArgs {
    folder: LitStr,
    ignore_files: Vec<LitStr>,
    backend: Option<Backend>,
}

enum Backend {
    Named(Ident),
    PackageName,
}

impl Parse for TestArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut folder = None;
        let mut ignore_files: Option<Vec<LitStr>> = None;
        let mut backend = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "folder" => folder = Some(input.parse()?),
                "ignore_files" => {
                    let content;
                    syn::bracketed!(content in input);
                    ignore_files = Some(
                        Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?
                            .into_iter()
                            .collect(),
                    );
                }
                "backend" => {
                    let name: Ident = input.parse()?;
                    backend = Some(if name == "package_name" {
                        Backend::PackageName
                    } else {
                        Backend::Named(name)
                    });
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "unsupported sample test argument",
                    ));
                }
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let folder = folder.ok_or_else(|| input.error("missing `folder`"))?;
        let ignore_files = ignore_files.ok_or_else(|| input.error("missing `ignore_files`"))?;
        if backend.is_none()
            && ignore_files
                .iter()
                .any(|filename| filename.value().contains("{backend}"))
        {
            return Err(input.error("`{backend}` in `ignore_files` requires `backend = ...`"));
        }

        Ok(Self {
            folder,
            ignore_files,
            backend,
        })
    }
}

fn expand_sample_tests(args: TestArgs, test_fn: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let backend = match &args.backend {
        None => None,
        Some(Backend::Named(name)) => Some(name.to_string()),
        Some(Backend::PackageName) => {
            let package_name = env::var("CARGO_PKG_NAME").map_err(|_| {
                syn::Error::new(
                    test_fn.sig.ident.span(),
                    "`backend = package_name` requires Cargo to set CARGO_PKG_NAME",
                )
            })?;
            let backend = package_name.strip_prefix("runtime_").ok_or_else(|| {
                syn::Error::new(
                    test_fn.sig.ident.span(),
                    "`backend = package_name` requires a runtime_* package name",
                )
            })?;
            Some(backend.to_owned())
        }
    };

    Ok(expand_sample_test_module(
        &args,
        &test_fn,
        backend.as_deref(),
    ))
}

fn expand_sample_test_module(
    args: &TestArgs,
    test_fn: &ItemFn,
    backend: Option<&str>,
) -> proc_macro2::TokenStream {
    let test_fn_name = &test_fn.sig.ident;
    let backend_name = backend.unwrap_or("default");
    let module_name = format_ident!("{}_{}", test_fn_name, backend_name);
    let backend_arg = backend.map(|backend| LitStr::new(backend, test_fn_name.span()));
    let ignore_files = args
        .ignore_files
        .iter()
        .map(|filename| filename.value().replace("{backend}", backend_name))
        .collect::<Vec<_>>();

    let folder = args.folder.value();
    let mut binding = WalkBuilder::new(Path::new(&folder));
    let mut file_walker = binding.hidden(false).ignore(false);
    file_walker = ignore_files.iter().fold(file_walker, |walker, file| {
        walker.add_custom_ignore_filename(file)
    });

    let mut tests = proc_macro2::TokenStream::new();
    for entry in file_walker.build().filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }

        let test_name = format_ident!("{}", get_test_name(path));
        let test_path = LitStr::new(&path.display().to_string(), test_fn_name.span());
        let test_call = if let Some(backend_arg) = &backend_arg {
            quote! { #test_fn_name(#test_path, #backend_arg) }
        } else {
            quote! { #test_fn_name(#test_path) }
        };
        tests.extend(quote! {
            #[cfg_attr(not(unix), ignore = "integration tests require Unix")]
            #[test]
            fn #test_name() {
                #test_call
            }
        });
    }

    quote! {
        mod #module_name {
            use super::*;
            #test_fn
            #tests
        }
    }
}

#[proc_macro_attribute]
pub fn gen_sample_crate_tests(input_args: TokenStream, input_fn: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input_args as TestArgs);
    let test_fn = parse_macro_input!(input_fn as ItemFn);
    match expand_sample_crate_tests(args, test_fn) {
        Ok(module) => TokenStream::from(module),
        Err(error) => TokenStream::from(error.into_compile_error()),
    }
}

fn expand_sample_crate_tests(
    args: TestArgs,
    test_fn: ItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    if args.backend.is_some() {
        return Err(syn::Error::new(
            test_fn.sig.ident.span(),
            "`backend` is not supported by `gen_sample_crate_tests`",
        ));
    }

    let mut tests = proc_macro2::TokenStream::new();
    let test_name_fn = &test_fn.sig.ident;
    let ignore_files = args
        .ignore_files
        .iter()
        .map(LitStr::value)
        .collect::<Vec<_>>();
    let mut binding = WalkBuilder::new(Path::new(&args.folder.value()));
    let mut file_walker = binding.hidden(false).ignore(false);
    file_walker = ignore_files.iter().fold(file_walker, |walker, file| {
        walker.add_custom_ignore_filename(file)
    });

    for entry in file_walker.build().filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if !path.join("Cargo.toml").exists() {
            continue;
        }

        let test_name = format_ident!("{}", get_test_name(path));
        let test_path = &path.display().to_string();
        tests.extend(quote! {
            #[cfg_attr(not(unix), ignore = "integration tests require Unix")]
            #[test]
            fn #test_name() {
                #test_name_fn(#test_path)
            }
        });
    }

    Ok(quote! {
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

    for item in &mut input.items {
        if let ImplItem::Fn(method) = item {
            let att = &mut method.attrs;
            let has_log_fn = att.iter().any(|attribute| {
                function_contains_log_fn(&attribute.to_token_stream().to_string())
            });

            if !has_log_fn {
                att.insert(0, instrument_stmt.clone());
            }
        }
    }
    let output = quote! {
        #input
    };
    output.into()
}

/// Conditionally derives serde's and rkyv's traits if the features are enabled.
#[proc_macro_attribute]
pub fn cond_derive_serde_rkyv(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let attrs: Vec<Attribute> = syn::parse_quote! {
        #[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
        #[cfg_attr(feature = "rkyv", derive(::rkyv::Archive, ::rkyv::Serialize, ::rkyv::Deserialize))]
    };
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    input.attrs.extend(attrs);
    input.to_token_stream().into()
}
