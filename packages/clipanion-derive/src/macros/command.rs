use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Attribute, DeriveInput, Expr, ExprLit, Fields, Ident, Lit, LitStr, Meta, Path};

use crate::{shared::expect_lit, utils::{to_lit_str, AttributeBag, CliAttributes, OptionBag}};

fn parse_documentation(value: String) -> proc_macro2::TokenStream {
    let lines
        = value.trim()
            .lines()
            .collect::<Vec<_>>();

    let has_details
        = lines.len() > 1;

    let mut lines_it
        = lines.into_iter();

    let description
        = to_lit_str(lines_it.next().expect("Missing first line"));

    let details = if has_details {
        let mut detail_lines
            = vec![String::new()];

        for line in lines_it {
            if line.is_empty() {
                detail_lines.push(String::new());
            } else {
                let trailing_line
                    = detail_lines.last_mut().unwrap();

                trailing_line.push(' ');
                trailing_line.push_str(line);
            }
        }

        let raw_details
            = detail_lines.join("\n\n");

        let trimmed_details
            = raw_details.trim();

        quote! {Some(::std::borrow::Cow::Borrowed(#trimmed_details))}
    } else {
        quote! {None}
    };

    quote! {Some(clipanion::core::Documentation {
        description: ::std::borrow::Cow::Borrowed(#description),
        details: #details,
    })}
}

/// A list of literals, borrowed: `["a", "b"]` is `Cow::Borrowed(&[Cow::Borrowed("a"), ...])`.
fn borrowed_strs<T: quote::ToTokens>(items: &[T]) -> proc_macro2::TokenStream {
    quote! {::std::borrow::Cow::Borrowed(&[#(::std::borrow::Cow::Borrowed(#items)),*])}
}

pub fn command_macro(args: TokenStream, mut input: DeriveInput) -> Result<TokenStream, syn::Error> {
    let syn::Data::Struct(struct_input) = &mut input.data else {
        panic!("Only structs are supported");
    };

    let partial_struct_ident
        = Ident::new(&format!("Partial{}", input.ident), Span::call_site());

    // The command's spec is a static: everything in it is known here, borrowed
    // from the binary, and takes no allocation to build.
    let mut components = vec![];
    let mut required_options = vec![];
    let mut hydraters = vec![];

    let mut command_cli_attributes
        = CliAttributes::extract(&mut input.attrs)?;

    let mut command_attribute_bag
        = syn::parse::<AttributeBag>(args)?;

    let command_category = command_cli_attributes
        .take_unique::<LitStr>("category")?
        .map_or(quote!{None}, |lit| quote!{Some(::std::borrow::Cow::Borrowed(#lit))});

    let command_documentation = command_cli_attributes
        .take_unique::<LitStr>("description")?
        .map(|lit_str| lit_str.value())
        .or_else(|| command_cli_attributes.documentation.clone())
        .map_or(quote! {None}, parse_documentation);

    let is_default = command_attribute_bag.take("default")
        .map(expect_lit!(Lit::Bool))
        .transpose()?
        .map(|lit| lit.value)
        .unwrap_or(false);

    let is_proxy = command_attribute_bag.take("proxy")
        .map(expect_lit!(Lit::Bool))
        .transpose()?
        .map(|lit| lit.value)
        .unwrap_or(false);

    // A proxy to a program, whose name never looks like an option: it only starts at a word that
    // isn't option-like, so the command's options (and `-h`, `--help`) come first. It's a proxy.
    let proxy_program = command_attribute_bag.take("proxy_program")
        .map(expect_lit!(Lit::Bool))
        .transpose()?
        .map(|lit| lit.value)
        .unwrap_or(false);

    let is_proxy = is_proxy || proxy_program;

    let explicit_positionals = command_attribute_bag.take("explicit_positionals")
        .map(expect_lit!(Lit::Bool))
        .transpose()?
        .map(|lit| lit.value)
        .unwrap_or(false);

    let mut paths_lits
        = command_cli_attributes.take_paths()?;

    command_attribute_bag.expect_empty()?;

    if is_default {
        paths_lits.push(vec![]);
    }

    if paths_lits.is_empty() {
        return Err(syn::Error::new_spanned(input.ident, "The command must have a path"));
    }

    let primary_path_index
        = paths_lits.iter()
            .enumerate()
            .max_by_key(|(_, path)| path.len())
            .map(|(index, _)| index)
            .unwrap_or(0);

    let primary_path
        = borrowed_strs(&paths_lits.remove(primary_path_index));

    let alias_paths = paths_lits.iter()
        .map(|path_lits| borrowed_strs(path_lits))
        .collect::<Vec<_>>();

    let mut partial_struct_members
        = vec![];
    let mut partial_struct_default_initializers
        = vec![];
    let mut initialization_members
        = vec![];

    for field in &mut struct_input.fields {
        let field_ident = &field.ident;
        let field_type = &field.ty;

        let mut internal_field_type = &field.ty;
        let mut is_option_type = false;

        // Option<Option<T>>; means the option supports both --foo without arguments (Some(None)), or --foo arg with arguments (Some(Some(...)))
        let mut is_option2_type = false;
        let mut is_vec_type = false;

        if let syn::Type::Path(type_path) = &internal_field_type {
            if &type_path.path.segments[0].ident == "Option" {
                let inner_type = &type_path.path.segments[0].arguments;
                if let syn::PathArguments::AngleBracketed(args) = inner_type {
                    if let syn::GenericArgument::Type(ty) = &args.args[0] {
                        internal_field_type = ty;
                        is_option_type = true;
                    }
                }
            }

            if let syn::Type::Path(type_path) = &internal_field_type {
                if &type_path.path.segments[0].ident == "Option" {
                    let inner_type = &type_path.path.segments[0].arguments;
                    if let syn::PathArguments::AngleBracketed(args) = inner_type {
                        if let syn::GenericArgument::Type(ty) = &args.args[0] {
                            internal_field_type = ty;
                            is_option2_type = true;
                        }
                    }
                }
            }

            if &type_path.path.segments[0].ident == "Vec" {
                let inner_type = &type_path.path.segments[0].arguments;
                if let syn::PathArguments::AngleBracketed(args) = inner_type {
                    if let syn::GenericArgument::Type(ty) = &args.args[0] {
                        internal_field_type = ty;
                        is_vec_type = true;
                    }
                }
            }
        }

        let mut cli_attributes
            = CliAttributes::extract(&mut field.attrs)?;

        if !explicit_positionals && !cli_attributes.attributes.contains_key("option") && !cli_attributes.attributes.contains_key("positional") {
            cli_attributes.attributes.insert("positional".to_string(), vec![Attribute {
                pound_token: Default::default(),
                style: syn::AttrStyle::Outer,
                bracket_token: Default::default(),
                meta: Meta::Path(Path::from(Ident::new("positional", Span::call_site()))),
            }]);
        }

        if let Some(mut option_bag) = cli_attributes.take_unique::<OptionBag>("option")? {
            let mut is_bool = false;
            let mut is_tuple = false;
            let mut is_counter = false;

            let mut item_count = 1usize;
            let mut min_len = 1usize;
            let mut extra_len = Some(0usize);

            if let syn::Type::Path(type_path) = &internal_field_type {
                if &type_path.path.segments[0].ident == "bool" {
                    is_bool = true;

                    min_len = 0;
                    extra_len = Some(0);
                }

                if let Some(_) = option_bag.attributes.take("counter") {
                    is_counter = true;

                    min_len = 0;
                    extra_len = Some(0);
                }
            }

            if let syn::Type::Tuple(tuple) = internal_field_type {
                let tuple_len = tuple.elems.len();

                is_tuple = true;

                item_count = tuple_len;
                min_len = tuple_len;
                extra_len = Some(0);
            }

            if is_vec_type {
                min_len = 1;
                extra_len = None;

                if let Some(min_len_expr) = option_bag.attributes.take("min_len") {
                    let min_len_lit
                        = expect_lit!(Lit::Int)(min_len_expr)?;

                    min_len = min_len_lit
                        .base10_parse::<usize>().unwrap_or(0);
                }
            }

            let documentation = option_bag.attributes.take("description")
                .map(expect_lit!(Lit::Str))
                .transpose()?
                .map(|lit_str| lit_str.value())
                .or_else(|| cli_attributes.documentation.clone())
                .map_or(quote! {None}, parse_documentation);

            let preferred_name = option_bag.path
                .iter().max_by_key(|name| name.len())
                .unwrap();

            let preferred_name_lit
                = to_lit_str(preferred_name);

            let aliases_lit = borrowed_strs(&option_bag.path
                .iter()
                .filter(|name| name != &preferred_name)
                .map(to_lit_str)
                .collect::<Vec<_>>());

            let min_len_lit = quote! {#min_len};
            let extra_len_lit = match extra_len {
                Some(extra_len) => quote! {Some(#extra_len)},
                None => quote! {None},
            };

            let mut value_converter = if is_vec_type {
                if is_tuple {
                    let mut tuple_fields = vec![];

                    for field in 0..item_count {
                        tuple_fields.push(quote! {
                            args.get(#field).map(|s| -> Result<_, clipanion::core::CommandError> {
                                s.value.parse().map_err(clipanion::details::handle_parse_error)
                            }).transpose()?.unwrap()
                        });
                    }

                    quote! {
                        args.chunks(#item_count).map(|chunk| -> Result<_, clipanion::core::CommandError> {
                            Ok((#(#tuple_fields),*))
                        }).collect::<Result<Vec<_>, _>>()?
                    }
                } else {
                    quote! {args.iter().map(|s| -> Result<_, clipanion::core::CommandError> {
                        s.value.parse().map_err(clipanion::details::handle_parse_error)
                    }).collect::<Result<Vec<_>, _>>()?}
                }
            } else if is_bool {
                quote! {Some(true)}
            } else if is_counter {
                quote! {Some(partial.#field_ident.unwrap_or_default() + 1)}
            } else if is_tuple {
                let mut tuple_fields
                    = vec![];

                for field in 0..item_count {
                    tuple_fields.push(quote! {
                        args.get(#field).map(|s| -> Result<_, clipanion::core::CommandError> {
                            s.value.parse().map_err(clipanion::details::handle_parse_error)
                        }).transpose()?.unwrap()
                    });
                }

                quote! {Some((#(#tuple_fields),*))}
            } else {
                quote! {args.first().map(|s| -> Result<_, clipanion::core::CommandError> {
                    s.value.parse().map_err(clipanion::details::handle_parse_error)
                }).transpose()?}
            };

            if is_option2_type {
                value_converter = quote! {
                    if args.is_empty() {
                        Some(None)
                    } else {
                        Some(#value_converter)
                    }
                };
            }

            let none_expr: Expr
                = syn::parse_str("None").unwrap();

            let default_value
                = option_bag.attributes.take("default")
                    .or_else(|| if is_option_type {Some(none_expr.clone())} else {None});

            let default_value_lit = match &default_value {
                Some(expr) => {
                    let expr_str = quote!{#expr}.to_string();
                    quote!{Some(::std::borrow::Cow::Borrowed(#expr_str))}
                },

                None => {
                    quote!{None}
                },
            };

            let is_required
                = default_value.is_none();

            if is_vec_type {
                partial_struct_members.push(quote! {
                    pub #field_ident: Vec<#internal_field_type>,
                });

                partial_struct_default_initializers.push(quote! {
                    #field_ident: std::default::Default::default(),
                });

                hydraters.push(quote! {
                    partial.#field_ident.extend(#value_converter);
                });

                initialization_members.push(quote! {
                    #field_ident: partial.#field_ident,
                });
            } else {
                hydraters.push(quote! {
                    partial.#field_ident = #value_converter;
                });

                // If the field is an option type we already assume it's an
                // optional option; we don't need to wrap it inside Option<T>
                let partial_struct_member_type = match is_option_type {
                    true => quote! {#field_type},
                    false => quote! {Option<#field_type>},
                };

                partial_struct_members.push(quote! {
                    pub #field_ident: #partial_struct_member_type,
                });

                partial_struct_default_initializers.push(quote! {
                    #field_ident: std::default::Default::default(),
                });

                let option_partial = match is_option_type {
                    true => quote! {Some(partial.#field_ident)},
                    false => quote! {partial.#field_ident},
                };

                let partial_to_ty = match default_value {
                    Some(expr) => quote! {#option_partial.unwrap_or_else(|| #expr)},
                    None => quote! {#option_partial.unwrap()},
                };

                initialization_members.push(quote! {
                    #field_ident: #partial_to_ty,
                });
            }

            if is_required {
                required_options.push(components.len());
            }

            components.push(quote! {
                clipanion::core::Component::Option(clipanion::core::OptionSpec {
                    primary_name: ::std::borrow::Cow::Borrowed(#preferred_name_lit),
                    aliases: #aliases_lit,
                    documentation: #documentation,
                    default_value: #default_value_lit,
                    is_hidden: false,
                    is_required: #is_required,
                    allow_binding: false,
                    allow_boolean: #is_option2_type,
                    min_len: #min_len_lit,
                    extra_len: #extra_len_lit,
                })
            });

            if !is_required {
                let no_option_names = option_bag.path
                    .iter()
                    .filter(|name| name.starts_with("--") && !name.starts_with("--no-"))
                    .map(|name| format!("--no-{}", &name[2..]))
                    .collect::<Vec<_>>();

                for no_option_name in no_option_names {
                    let no_option_name_lit
                        = LitStr::new(&no_option_name, Span::call_site());

                    if is_vec_type || (is_option_type && !is_bool) {
                        hydraters.push(quote! {
                            partial.#field_ident = Default::default();
                        });
                    } else {
                        if is_option_type || is_bool || is_counter {
                            hydraters.push(quote! {
                                partial.#field_ident = Some(Default::default());
                            });
                        } else {
                            hydraters.push(quote! {
                                partial.#field_ident = None;
                            });
                        }
                    }

                    components.push(quote! {
                        clipanion::core::Component::Option(clipanion::core::OptionSpec {
                            primary_name: ::std::borrow::Cow::Borrowed(#no_option_name_lit),
                            aliases: ::std::borrow::Cow::Borrowed(&[]),
                            documentation: None,
                            default_value: None,
                            is_hidden: true,
                            is_required: false,
                            allow_binding: false,
                            allow_boolean: false,
                            min_len: 0,
                            extra_len: Some(0),
                        })
                    });
                }
            }

            option_bag.attributes.expect_empty()?;
        } else if let Some(mut positional_bag) = cli_attributes.take_unique::<AttributeBag>("positional")? {
            let documentation = positional_bag.take("description")
                .map(expect_lit!(Lit::Str))
                .transpose()?
                .map(|lit_str| lit_str.value())
                .or_else(|| cli_attributes.documentation.clone())
                .map_or(quote! {None}, parse_documentation);

            let is_prefix = positional_bag.take("is_prefix")
                .map(expect_lit!(Lit::Bool))
                .transpose()?
                .map(|lit| lit.value())
                .unwrap_or_default();

            let completer = positional_bag.take("completer");

            let field_name_upper = field.ident.as_ref().unwrap()
                .to_string()
                .to_uppercase();

            if is_vec_type {
                // How many values the list needs (`#[cli::positional(min_len = 1)]`).
                let min_len = positional_bag.take("min_len")
                    .map(expect_lit!(Lit::Int))
                    .transpose()?
                    .map(|lit| lit.base10_parse::<usize>())
                    .transpose()?
                    .unwrap_or(0);

                partial_struct_members.push(quote! {
                    pub #field_ident: Vec<#internal_field_type>,
                });

                partial_struct_default_initializers.push(quote! {
                    #field_ident: std::default::Default::default(),
                });

                hydraters.push(quote! {
                    let value = args.iter()
                        .map(|arg| arg.value.parse().map_err(clipanion::details::handle_parse_error))
                        .collect::<Result<Vec<_>, _>>()?;

                    partial.#field_ident = value;
                });

                initialization_members.push(quote! {
                    #field_ident: partial.#field_ident,
                });

                let completer_expr = match &completer {
                    Some(expr) => quote! { Some(#expr) },
                    None => quote! { None },
                };

                components.push(quote! {
                    clipanion::core::Component::Positional(clipanion::core::PositionalSpec::Dynamic {
                        name: ::std::borrow::Cow::Borrowed(#field_name_upper),
                        documentation: #documentation,
                        min_len: #min_len,
                        extra_len: None,
                        is_prefix: #is_prefix,
                        is_proxy: #is_proxy,
                        proxy_program: #proxy_program,
                        completer: #completer_expr,
                    })
                });
            } else {
                partial_struct_members.push(quote! {
                    pub #field_ident: Option<#field_type>,
                });

                partial_struct_default_initializers.push(quote! {
                    #field_ident: std::default::Default::default(),
                });

                if is_option_type {
                    hydraters.push(quote! {
                        let positional = args.first().unwrap();

                        let value = positional.value.parse()
                            .map_err(clipanion::details::handle_parse_error)?;

                        partial.#field_ident = Some(Some(value));
                    });

                    initialization_members.push(quote! {
                        #field_ident: partial.#field_ident.unwrap_or_default(),
                    });
                } else {
                    hydraters.push(quote! {
                        let positional = args.first().unwrap();

                        let value = positional.value.parse()
                            .map_err(clipanion::details::handle_parse_error)?;

                        partial.#field_ident = Some(value);
                    });

                    initialization_members.push(quote! {
                        #field_ident: partial.#field_ident.unwrap(),
                    });
                }

                let (min_len, extra_len) = match is_option_type {
                    true => (quote!{0}, quote!{Some(1)}),
                    false => (quote!{1}, quote!{Some(0)}),
                };

                let completer_expr = match &completer {
                    Some(expr) => quote! { Some(#expr) },
                    None => quote! { None },
                };

                components.push(quote! {
                    clipanion::core::Component::Positional(clipanion::core::PositionalSpec::Dynamic {
                        name: ::std::borrow::Cow::Borrowed(#field_name_upper),
                        documentation: #documentation,
                        min_len: #min_len,
                        extra_len: #extra_len,
                        is_prefix: #is_prefix,
                        is_proxy: false,
                        proxy_program: false,
                        completer: #completer_expr,
                    })
                });
            }

            positional_bag.expect_empty()?;
        }
    }

    if let Fields::Named(fields) = &mut struct_input.fields {
        fields.named.push(syn::parse_quote! {cli_environment: clipanion::advanced::Environment});
        fields.named.push(syn::parse_quote! {cli_path: Vec<String>});
    }

    let struct_name
        = &input.ident;

    let expanded = quote! {
        #input

        #[derive(Debug)]
        pub struct #partial_struct_ident {
            pub cli_environment: clipanion::advanced::Environment,
            pub cli_path: Vec<String>,

            #(#partial_struct_members)*
        }

        impl #partial_struct_ident {
            pub fn new(environment: &clipanion::advanced::Environment, path: Vec<String>) -> Self {
                Self {
                    cli_environment: environment.clone(),
                    cli_path: path,

                    #(#partial_struct_default_initializers)*
                }
            }
        }

        impl ::core::convert::TryFrom<#partial_struct_ident> for #struct_name {
            type Error = ::clipanion::core::CommandError;

            fn try_from(partial: #partial_struct_ident) -> Result<Self, ::clipanion::core::CommandError> {
                Ok(Self {
                    cli_environment: partial.cli_environment,
                    cli_path: partial.cli_path,

                    #(#initialization_members)*
                })
            }
        }

        impl clipanion::details::CommandController for #struct_name {
            type Partial = #partial_struct_ident;

            fn command_usage(opts: clipanion::core::CommandUsageOptions) -> Result<clipanion::core::CommandUsageResult<'static>, clipanion::core::BuildError> {
                Ok(#struct_name::command_spec()?.usage())
            }

            fn command_spec() -> Result<&'static clipanion::core::CommandSpec, clipanion::core::BuildError> {
                static COMMAND_SPEC: clipanion::core::CommandSpec = clipanion::core::CommandSpec {
                    primary_path: #primary_path,
                    aliases: ::std::borrow::Cow::Borrowed(&[#(#alias_paths),*]),
                    category: #command_category,
                    documentation: #command_documentation,
                    examples: ::std::borrow::Cow::Borrowed(&[]),
                    components: ::std::borrow::Cow::Borrowed(&[#(#components),*]),
                    required_options: ::std::borrow::Cow::Borrowed(&[#(#required_options),*]),
                };

                Ok(&COMMAND_SPEC)
            }

            fn hydrate_from_state(environment: &clipanion::advanced::Environment, state: &clipanion::core::State) -> Result<Self::Partial, clipanion::core::CommandError> {
                let mut partial
                    = Self::Partial::new(environment, state.path.iter().map(|s| s.to_string()).collect());

                let FNS: &[fn(&mut Self::Partial, &Vec<clipanion::core::UserArg<'_>>) -> Result<(), clipanion::core::CommandError>] = &[
                    #(|partial, args| {
                        #hydraters
                        Ok(())
                    }),*
                ];

                for (index, args) in &state.option_values {
                    FNS[*index](&mut partial, args)?;
                }

                for (index, args) in &state.positional_values {
                    FNS[*index](&mut partial, args)?;
                }

                Ok(partial)
            }
        }
    };

    Ok(TokenStream::from(expanded))
}
