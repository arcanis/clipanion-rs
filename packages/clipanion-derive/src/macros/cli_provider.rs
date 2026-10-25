extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Fields};

pub fn cli_provider_macro(_args: TokenStream, mut input: DeriveInput) -> Result<TokenStream, syn::Error> {
    let syn::Data::Enum(enum_input) = &mut input.data else {
        panic!("Only enums are supported");
    };

    let enum_ident
        = &input.ident;

    let mut variant_tys
        = vec![];

    for variant in &enum_input.variants {
        let Fields::Unnamed(fields) = &variant.fields else {
            panic!("Only unnamed fields are supported");
        };

        if fields.unnamed.len() != 1 {
            panic!("Only one field is supported");
        }

        let variant_ty
            = &fields.unnamed.first().unwrap().ty;

        variant_tys.push(variant_ty.clone());
    }
    
    Ok(TokenStream::from(quote! {
        #input

        impl clipanion::details::CommandProvider for #enum_ident {
            type Command = #enum_ident;

            fn command_usage(command_index: usize, opts: clipanion::core::CommandUsageOptions) -> Result<clipanion::core::CommandUsageResult<'static>, clipanion::core::BuildError> {
                use clipanion::details::CommandController;

                const FNS: &[fn(clipanion::core::CommandUsageOptions) -> Result<clipanion::core::CommandUsageResult<'static>, clipanion::core::BuildError>] = &[
                    #(<#variant_tys>::command_usage,)*
                ];

                FNS[command_index](opts)
            }

            fn registered_commands() -> Result<Vec<&'static clipanion::core::CommandSpec>, clipanion::core::BuildError> {
                use clipanion::details::CommandController;

                Ok(vec![
                    #(<#variant_tys>::command_spec()?,)*
                ])
            }

            fn parse_args<'args>(builder: &clipanion::core::CliBuilder<'static>, environment: &'args clipanion::advanced::Environment) -> Result<clipanion::core::SelectionResult<'static, 'args, <#enum_ident as clipanion::details::CliEnums>::PartialEnum>, clipanion::core::Error<'args>> where #enum_ident: clipanion::details::CliEnums {
                let argv
                    = environment.argv.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>();

                let mut selector
                    = builder.run(&argv)?;

                const FNS: &[fn(&clipanion::advanced::Environment, &clipanion::core::State<'_>) -> Result<<#enum_ident as ::clipanion::details::CliEnums>::PartialEnum, clipanion::core::CommandError>] = &[
                    #(|environment, state| {
                        use clipanion::details::CommandController;

                        let partial
                            = <#variant_tys>::hydrate_from_state(environment, state)?;

                        Ok(partial.into())
                    },)*
                ];

                selector.resolve_state(|state| {
                    let command
                        = FNS[state.context_id](environment, state)?;

                    Ok(command.into())
                })
            }

            fn build_cli() -> Result<clipanion::core::CliBuilder<'static>, clipanion::core::BuildError> {
                use clipanion::details::CommandController;

                let mut builder
                    = clipanion::core::CliBuilder::new();

                #(builder.add_command(<#variant_tys>::command_spec()?);)*

                if clipanion::core::is_debug_enabled() {
                    println!("========== CLI State Machine ==========");
                    println!("{:?}", builder.compile());
                }

                Ok(builder)
            }
        }
    }))
}
