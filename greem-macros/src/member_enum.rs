//! `#[derive(greem::Abstract)]`: a member enum's `Completes` impls, one owned
//! and one by-reference pair per abstract type it targets.

use crate::parse_options;
use proc_macro2::{Group, Span, TokenStream as TokenStream2, TokenTree};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Error, Fields, GenericParam, Generics, Ident, Lifetime,
    LifetimeParam, LitStr, Path, Result, Type, WherePredicate,
};

struct Target {
    schema: Path,
    tag: Ident,
}

struct Variant {
    ident: Ident,
    payload: Type,
    /// The member's tag identifier, spanned where the member is named.
    member: Ident,
}

fn targets(attrs: &[Attribute], enum_ident: &Ident) -> Result<Vec<Target>> {
    let attrs: Vec<&Attribute> = attrs
        .iter()
        .filter(|a| a.path().is_ident("greem"))
        .collect();
    let mut out = Vec::new();
    for attr in &attrs {
        let mut schema = None;
        let mut type_name = None;
        attr.parse_args_with(|input: ParseStream| {
            parse_options(input, "`greem::Abstract` option", |key, input| {
                match key {
                    "schema" => schema = Some(input.parse::<Path>()?),
                    "type" => type_name = Some(input.parse::<LitStr>()?),
                    _ => return Ok(false),
                }
                Ok(true)
            })
        })?;
        if type_name.is_none() && attrs.len() > 1 {
            return Err(Error::new(
                attr.span(),
                "an enum that targets several abstract types names each with `type = \"...\"`",
            ));
        }
        out.push((schema, type_name));
    }
    if out.is_empty() {
        out.push((None, None));
    }
    Ok(out
        .into_iter()
        .map(|(schema, type_name)| Target {
            schema: schema.unwrap_or_else(|| syn::parse_quote!(crate::schema)),
            tag: match type_name {
                Some(name) => greem_core::ident(&name.value(), name.span()),
                None => greem_core::ident(&enum_ident.unraw().to_string(), enum_ident.span()),
            },
        })
        .collect())
}

fn member_name(variant: &syn::Variant) -> Result<(String, Span)> {
    let mut name = None;
    for attr in variant.attrs.iter().filter(|a| a.path().is_ident("greem")) {
        attr.parse_args_with(|input: ParseStream| {
            parse_options(input, "`greem::Abstract` variant option", |key, input| {
                if key != "as" {
                    return Ok(false);
                }
                let value = input.parse::<LitStr>()?;
                name = Some((value.value(), value.span()));
                Ok(true)
            })
        })?;
    }
    Ok(name.unwrap_or_else(|| (variant.ident.unraw().to_string(), variant.ident.span())))
}

fn variants(input: &DeriveInput) -> Result<Vec<Variant>> {
    let Data::Enum(data) = &input.data else {
        return Err(Error::new(
            input.ident.span(),
            "`#[derive(greem::Abstract)]` goes on an enum",
        ));
    };
    if data.variants.is_empty() {
        return Err(Error::new(
            input.ident.span(),
            "a member enum needs at least one variant",
        ));
    }
    if data.variants.len() > usize::from(u8::MAX) + 1 {
        return Err(Error::new(
            input.ident.span(),
            "a member enum has at most 256 variants",
        ));
    }
    data.variants
        .iter()
        .map(|variant| {
            let payload = match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => fields.unnamed[0].ty.clone(),
                _ => {
                    return Err(Error::new(
                        variant.span(),
                        "a member enum variant holds its member value in one unnamed field, as `User(MyUser)`",
                    ));
                }
            };
            let (name, span) = member_name(variant)?;
            Ok(Variant {
                ident: variant.ident.clone(),
                payload,
                member: greem_core::ident(&name, span),
            })
        })
        .collect()
}

/// `tokens` with every span set to `span`, so a witness that fails points
/// at the variant that named the member.
fn respan(tokens: TokenStream2, span: Span) -> TokenStream2 {
    tokens
        .into_iter()
        .map(|mut tree| {
            if let TokenTree::Group(group) = &tree {
                let mut inner = Group::new(group.delimiter(), respan(group.stream(), span));
                inner.set_span(span);
                tree = TokenTree::Group(inner);
            }
            tree.set_span(span);
            tree
        })
        .collect()
}

/// The impl's generics: the enum's, after `by_ref` and before the context
/// parameter `__C`, with `bounds` added to the where clause.
fn impl_generics(
    generics: &Generics,
    by_ref: Option<&Lifetime>,
    bounds: Vec<WherePredicate>,
) -> Generics {
    let mut generics = generics.clone();
    if let Some(lifetime) = by_ref {
        generics.params.insert(
            0,
            GenericParam::Lifetime(LifetimeParam::new(lifetime.clone())),
        );
    }
    generics.params.push(syn::parse_quote!(__C));
    generics.make_where_clause().predicates.extend(bounds);
    generics
}

/// `tag`'s `Completes` impl for the enum, or for `&'__r` of it when `by_ref`
/// is given: each variant completes as its member under its own leaf step.
fn completes(
    input: &DeriveInput,
    tag: &TokenStream2,
    variants: &[Variant],
    members: &[TokenStream2],
    by_ref: Option<&Lifetime>,
) -> TokenStream2 {
    let enum_ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let reference = by_ref.map(|lifetime| quote!(&#lifetime));
    let self_ty = quote!(#reference #enum_ident #ty_generics);
    // Matching `&Enum` binds `&Payload`; matching `&&Enum` through one deref
    // binds `&Payload` too, borrowed once more for `&Payload: Outputs`.
    let (deref, borrow) = match by_ref {
        Some(_) => (quote!(*), quote!(&)),
        None => (quote!(), quote!()),
    };
    let mut bounds: Vec<WherePredicate> = Vec::new();
    let mut walks = Vec::new();
    let mut first_errors = Vec::new();
    let mut buckets = Vec::new();
    let mut partitions = Vec::new();
    let mut completions = Vec::new();
    let mut references = Vec::new();
    for (step, (variant, member)) in variants.iter().zip(members).enumerate() {
        let step = step as u8;
        let ident = &variant.ident;
        let payload = &variant.payload;
        let payload = quote!(#reference #payload);
        let outputs = quote!(<#payload as ::greem::Outputs<#member, __C>>);
        let values = format_ident!("__values{step}");
        let positions = format_ident!("__positions{step}");
        bounds.push(syn::parse_quote!(
            #payload: ::greem::Outputs<#member, __C> + ::core::marker::Send
        ));
        walks.push(quote! {
            ::greem::__private::walk_as::<#member, #payload, __C>(#step, w, node, leaf)?;
        });
        first_errors.push(quote! {
            #enum_ident::#ident(value) => #outputs::__first_error(#borrow value, indices, wanted),
        });
        buckets.push(quote! {
            let mut #values = ::std::vec::Vec::new();
            let mut #positions = ::std::vec::Vec::new();
        });
        partitions.push(quote! {
            #enum_ident::#ident(value) => {
                #values.push(::greem::As::new(value));
                #positions.push(pos);
            }
        });
        completions.push(quote! {
            if !#values.is_empty() {
                ::greem::__private::complete_as::<#member, #payload, __C>(#step, #values, #positions, cc);
            }
        });
        references.push(quote! {
            #enum_ident::#ident(value) => #outputs::__reference(value, &rc.with_leaf(#step)),
        });
    }
    bounds.push(syn::parse_quote!(
        __C: ::core::marker::Send + ::core::marker::Sync + 'static
    ));
    let generics = impl_generics(&input.generics, by_ref, bounds);
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    quote! {
        #[automatically_derived]
        impl #impl_generics ::greem::__private::Completes<#self_ty, __C> for #tag #where_clause {
            fn walk<'__a>(
                w: &mut ::greem::__private::Walker<'_, __C>,
                node: ::greem::__private::NodeId,
                leaf: &::greem::__private::Leaf,
            ) -> ::core::result::Result<(), ::greem::__private::Abort>
            where
                #self_ty: '__a,
                __C: '__a,
            {
                #(#walks)*
                ::core::result::Result::Ok(())
            }

            fn first_error(
                value: &#self_ty,
                indices: &mut ::std::vec::Vec<::core::primitive::u32>,
                wanted: &dyn ::core::ops::Fn(&[::core::primitive::u32]) -> ::core::primitive::bool,
            ) -> ::core::option::Option<::greem::Error> {
                match #deref value {
                    #(#first_errors)*
                }
            }

            fn complete<'__a>(
                values: ::std::vec::Vec<#self_ty>,
                positions: ::std::vec::Vec<::greem::__private::Pos>,
                cc: &mut ::greem::__private::Completion<'__a, '_, __C>,
            ) where
                #self_ty: '__a,
                __C: '__a,
            {
                #(#buckets)*
                for (value, pos) in ::core::iter::Iterator::zip(values.into_iter(), positions) {
                    match value {
                        #(#partitions)*
                    }
                }
                #(#completions)*
            }

            ::greem::__private::reference! {
                fn reference<'__v, '__s: '__v>(
                    value: #self_ty,
                    rc: &::greem::__private::RefCompletion<'__s, __C>,
                ) -> ::greem::__private::futures::future::BoxFuture<'__v, ::greem::__private::RefValue>
                where
                    #self_ty: '__v,
                    __C: '__v,
                {
                    match value {
                        #(#references)*
                    }
                }
            }
        }
    }
}

pub(crate) fn expand(input: DeriveInput) -> Result<TokenStream2> {
    let targets = targets(&input.attrs, &input.ident)?;
    let variants = variants(&input)?;
    let by_ref = Lifetime::new("'__r", Span::call_site());
    let mut out = Vec::new();
    for Target { schema, tag } in &targets {
        let tag = quote!(#schema::types::#tag);
        let members: Vec<TokenStream2> = variants
            .iter()
            .map(|v| {
                let member = &v.member;
                quote!(#schema::types::#member)
            })
            .collect();
        let witnesses = variants.iter().zip(&members).map(|(v, member)| {
            respan(
                quote!(::greem::__private::member::<#tag, #member>();),
                v.member.span(),
            )
        });
        out.push(quote!(const _: () = { #(#witnesses)* };));
        out.push(completes(&input, &tag, &variants, &members, None));
        out.push(completes(&input, &tag, &variants, &members, Some(&by_ref)));
    }
    Ok(quote!(#(#out)*))
}

#[cfg(test)]
mod tests {
    use super::expand;
    use quote::quote;

    fn error(input: proc_macro2::TokenStream) -> String {
        match expand(syn::parse2(input).unwrap()) {
            Ok(tokens) => panic!("expanded: {tokens}"),
            Err(error) => error.to_string(),
        }
    }

    fn flat(input: proc_macro2::TokenStream) -> String {
        let code = expand(syn::parse2(input).unwrap()).unwrap().to_string();
        code.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn variants_hold_one_unnamed_field() {
        for variant in [
            quote!(User),
            quote!(User { user: MyUser }),
            quote!(User(MyUser, Extra)),
        ] {
            let message = error(quote!(enum Hit { #variant, Post(MyPost) }));
            assert!(
                message.contains("one unnamed field"),
                "{variant}: {message}"
            );
        }
    }

    #[test]
    fn only_non_empty_enums_derive() {
        assert!(
            error(quote!(
                struct Hit(MyUser);
            ))
            .contains("goes on an enum")
        );
        assert!(
            error(quote!(
                enum Hit {}
            ))
            .contains("at least one variant")
        );
    }

    #[test]
    fn several_targets_name_each_type() {
        let message = error(quote! {
            #[greem(type = "Node")]
            #[greem(schema = crate::schema)]
            enum Hit { User(MyUser) }
        });
        assert!(message.contains("names each with `type"), "{message}");
    }

    #[test]
    fn unknown_options_are_rejected() {
        let message = error(quote! {
            #[greem(context = App)]
            enum Hit { User(MyUser) }
        });
        assert!(
            message.contains("unknown `greem::Abstract` option `context`"),
            "{message}"
        );
        let message = error(quote! {
            enum Hit { #[greem(name = "User")] Person(MyUser) }
        });
        assert!(
            message.contains("unknown `greem::Abstract` variant option `name`"),
            "{message}"
        );
    }

    #[test]
    fn each_variant_witnesses_its_member_on_every_target() {
        let code = flat(quote! {
            #[greem(type = "Node")]
            #[greem(schema = other::schema, type = "SearchResult")]
            enum Hit {
                #[greem(as = "User")]
                Person(MyUser),
                r#Post(MyPost),
            }
        });
        for witness in [
            "member::<crate::schema::types::Node,crate::schema::types::User>()",
            "member::<crate::schema::types::Node,crate::schema::types::Post>()",
            "member::<other::schema::types::SearchResult,other::schema::types::User>()",
            "member::<other::schema::types::SearchResult,other::schema::types::Post>()",
        ] {
            assert!(code.contains(witness), "{witness}: {code}");
        }
    }

    #[test]
    fn enum_generics_are_forwarded() {
        let code = flat(quote! {
            #[greem(type = "SearchResult")]
            enum Hit<'a, T: Clone> where T: Sync {
                User(&'a T),
                Post(Box<T>),
            }
        });
        assert!(
            code.contains("impl<'a,T:Clone,__C>::greem::__private::Completes<Hit<'a,T>,__C>"),
            "{code}"
        );
        assert!(
            code.contains(
                "impl<'__r,'a,T:Clone,__C>::greem::__private::Completes<&'__rHit<'a,T>,__C>"
            ),
            "{code}"
        );
        assert!(
            code.contains("whereT:Sync,&'aT:::greem::Outputs<"),
            "{code}"
        );
        assert!(code.contains("&'__r&'aT:::greem::Outputs<"), "{code}");
    }
}
