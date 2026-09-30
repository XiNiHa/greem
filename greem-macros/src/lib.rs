//! Procedural macros for greem: `#[greem::object]` (per-type resolver sugar)
//! and the `Abstract` derive placeholder.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{
    Attribute, Error, FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, ItemImpl, Lifetime,
    LitStr, Pat, Path, PathArguments, Result, ReturnType, Token, Type, TypePath, TypeReference,
};

struct ObjectArgs {
    schema: Path,
    type_name: Option<String>,
    context: Type,
}

impl Parse for ObjectArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut schema: Option<Path> = None;
        let mut type_name = None;
        let mut context: Option<Type> = None;
        while !input.is_empty() {
            let key = Ident::parse_any(input)?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "schema" => schema = Some(input.parse()?),
                "type" => type_name = Some(input.parse::<LitStr>()?.value()),
                "context" => context = Some(input.parse()?),
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!("unknown `greem::object` option `{other}`"),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(ObjectArgs {
            schema: schema.unwrap_or_else(|| syn::parse_quote!(crate::schema)),
            type_name,
            context: context.unwrap_or_else(|| syn::parse_quote!(())),
        })
    }
}

/// Generates one `greem::Resolver` impl per method of an inherent impl block.
#[proc_macro_attribute]
pub fn object(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as ObjectArgs);
    let item = syn::parse_macro_input!(item as ItemImpl);
    match expand_object(args, item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

#[proc_macro_derive(Abstract, attributes(greem))]
pub fn derive_abstract(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);
    Error::new(
        input.ident.span(),
        "`#[derive(greem::Abstract)]` is not implemented yet; use `greem::Either`/`greem::As`",
    )
    .to_compile_error()
    .into()
}

#[derive(Default)]
struct GreemAttrs {
    name: Option<String>,
    hints: Option<String>,
    plan: Option<String>,
}

fn take_greem_attrs(attrs: &mut Vec<Attribute>) -> Result<GreemAttrs> {
    let mut out = GreemAttrs::default();
    let mut kept = Vec::new();
    for attr in attrs.drain(..) {
        if !attr.path().is_ident("greem") {
            kept.push(attr);
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let value = meta.value()?.parse::<LitStr>()?.value();
            if meta.path.is_ident("name") {
                out.name = Some(value);
            } else if meta.path.is_ident("hints") {
                out.hints = Some(value);
            } else if meta.path.is_ident("plan") {
                out.plan = Some(value);
            } else {
                return Err(meta.error("expected `name`, `hints` or `plan`"));
            }
            Ok(())
        })?;
    }
    *attrs = kept;
    Ok(out)
}

fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for (i, ch) in name.chars().enumerate() {
        if ch == '_' && i != 0 {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn marker_ident(name: &str, span: Span) -> Ident {
    greem_core::ident(name, span)
}

enum Receiver {
    Object,
    Set,
}

struct Method {
    ident: Ident,
    field: String,
    receiver: Receiver,
    tail: usize,
    is_async: bool,
    /// The element output type (per object) with output lifetimes rewritten to `'obj`.
    output: Type,
    /// Whether the method returns a `Result` whose error is passed through.
    fallible: bool,
    /// The method's `#[cfg]` and `#[cfg_attr]` attributes, repeated on its generated impl.
    cfgs: Vec<Attribute>,
    span: Span,
}

fn result_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(TypePath { qself: None, path }) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    if args.args.is_empty() || args.args.len() > 2 {
        return None;
    }
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

fn vec_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(TypePath { qself: None, path }) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    if last.ident != "Vec" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

struct LifetimeRewriter {
    named: Vec<String>,
}

/// Lets methods keep writing `ctx: &Context<App>`: `Context<'req, C>` needs
/// its lifetime spelled out inside `async fn` signatures, so `'_` is inserted.
struct ContextLifetime;

impl VisitMut for ContextLifetime {
    fn visit_type_path_mut(&mut self, ty: &mut TypePath) {
        if let Some(last) = ty.path.segments.last_mut()
            && last.ident == "Context"
            && let PathArguments::AngleBracketed(args) = &mut last.arguments
            && args.args.len() == 1
            && matches!(args.args.first(), Some(GenericArgument::Type(_)))
        {
            let elided = Lifetime::new("'_", Span::call_site());
            args.args.insert(0, GenericArgument::Lifetime(elided));
        }
        syn::visit_mut::visit_type_path_mut(self, ty);
    }
}

impl VisitMut for LifetimeRewriter {
    fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
        let name = lifetime.ident.to_string();
        if name == "_" || self.named.contains(&name) {
            *lifetime = Lifetime::new("'obj", lifetime.span());
        }
    }

    fn visit_type_reference_mut(&mut self, reference: &mut TypeReference) {
        if reference.lifetime.is_none() {
            reference.lifetime = Some(Lifetime::new("'obj", reference.span()));
        }
        syn::visit_mut::visit_type_reference_mut(self, reference);
    }
}

fn analyze(method: &ImplItemFn, attrs: &GreemAttrs) -> Result<Method> {
    let sig = &method.sig;
    let span = sig.ident.span();
    let mut inputs = sig.inputs.iter();
    let receiver = match inputs.next() {
        Some(FnArg::Receiver(receiver)) => {
            if receiver.reference.is_none() || receiver.mutability.is_some() {
                return Err(Error::new(
                    receiver.span(),
                    "resolver methods take `&self` (or a receiver-less `parents` first parameter)",
                ));
            }
            Receiver::Object
        }
        Some(FnArg::Typed(typed)) => match &*typed.pat {
            Pat::Ident(pat) if pat.ident == "parents" => Receiver::Set,
            _ => {
                return Err(Error::new(
                    typed.span(),
                    "resolver methods take `&self` or a first parameter named `parents` for set-based resolution",
                ));
            }
        },
        None => {
            return Err(Error::new(
                span,
                "resolver methods take `&self` or a `parents` first parameter",
            ));
        }
    };
    let tail = inputs.count();
    if tail > 2 {
        return Err(Error::new(
            span,
            "resolver methods take at most `(args, ctx)` after the receiver",
        ));
    }
    let returned: Type = match &sig.output {
        ReturnType::Default => syn::parse_quote!(()),
        ReturnType::Type(_, ty) => (**ty).clone(),
    };
    let (mut output, fallible) = match result_inner(&returned) {
        Some(inner) => (inner.clone(), true),
        None => (returned.clone(), false),
    };
    if matches!(receiver, Receiver::Set) {
        output = match vec_inner(&output) {
            Some(inner) => inner.clone(),
            None => {
                return Err(Error::new(
                    returned.span(),
                    "set-based resolver methods return `Vec<Output>` (optionally inside `Result`)",
                ));
            }
        };
    }
    let named = sig
        .generics
        .lifetimes()
        .map(|l| l.lifetime.ident.to_string())
        .collect();
    LifetimeRewriter { named }.visit_type_mut(&mut output);
    let field = attrs
        .name
        .clone()
        .unwrap_or_else(|| camel_case(&sig.ident.unraw().to_string()));
    Ok(Method {
        ident: sig.ident.clone(),
        field,
        receiver,
        tail,
        is_async: sig.asyncness.is_some(),
        output,
        fallible,
        cfgs: Vec::new(),
        span,
    })
}

fn expand_object(args: ObjectArgs, mut item: ItemImpl) -> Result<TokenStream2> {
    if item.trait_.is_some() {
        return Err(Error::new(
            item.span(),
            "`#[greem::object]` goes on an inherent impl block",
        ));
    }
    let self_ty = (*item.self_ty).clone();
    let type_name = match args.type_name {
        Some(name) => name,
        None => match &self_ty {
            Type::Path(TypePath { path, .. }) => path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default(),
            _ => {
                return Err(Error::new(
                    self_ty.span(),
                    "give the GraphQL type name with `type = \"...\"`",
                ));
            }
        },
    };
    let schema = args.schema;
    let context = args.context;
    let type_ident = marker_ident(&type_name, Span::call_site());

    let mut methods = Vec::new();
    let mut hints: Vec<(String, Ident, Vec<Attribute>)> = Vec::new();
    let mut plans: Vec<(String, Ident, Vec<Attribute>)> = Vec::new();
    for entry in &mut item.items {
        let ImplItem::Fn(method) = entry else {
            continue;
        };
        for input in &mut method.sig.inputs {
            if let FnArg::Typed(arg) = input {
                ContextLifetime.visit_type_mut(&mut arg.ty);
            }
        }
        let attrs = take_greem_attrs(&mut method.attrs)?;
        let cfgs: Vec<Attribute> = method
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
            .cloned()
            .collect();
        if attrs.hints.is_some() || attrs.plan.is_some() {
            if let Some(field) = attrs.hints {
                hints.push((field, method.sig.ident.clone(), cfgs.clone()));
            }
            if let Some(field) = attrs.plan {
                plans.push((field, method.sig.ident.clone(), cfgs));
            }
            continue;
        }
        let mut analyzed = analyze(method, &attrs)?;
        analyzed.cfgs = cfgs;
        methods.push(analyzed);
    }

    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    let mut impls = Vec::new();
    for method in &methods {
        let marker = marker_ident(&method.field, method.span);
        let marker_path = quote!(#schema::#type_ident::#marker);
        let name = &method.ident;
        let output = &method.output;
        let call_args: Vec<TokenStream2> = match method.tail {
            0 => vec![],
            1 => vec![quote!(args)],
            _ => vec![quote!(args), quote!(ctx)],
        };
        let await_ = if method.is_async {
            quote!(.await)
        } else {
            quote!()
        };
        let body = match method.receiver {
            Receiver::Object => {
                let call = quote!(<Self>::#name(parent, #(#call_args),*) #await_);
                quote! {
                    let outputs = ::greem::__private::futures::future::join_all(
                        parents.iter().map(|parent| {
                            let parent: &'obj Self = *parent;
                            async move { #call }
                        }),
                    )
                    .await;
                    ::core::result::Result::Ok(outputs)
                }
            }
            Receiver::Set => {
                let call = quote!(<Self>::#name(parents, #(#call_args),*) #await_);
                if method.fallible {
                    quote!(#call)
                } else {
                    quote!(::core::result::Result::Ok(#call))
                }
            }
        };
        // Only a fallible per-object method wraps its output: infallible ones
        // keep their type, so slices and `Streamed` complete as they would set-based.
        let output_ty = match method.receiver {
            Receiver::Object if method.fallible => {
                quote!(::core::result::Result<#output, ::greem::Error>)
            }
            _ => quote!(#output),
        };
        // Each hook keeps its own `cfg`s, so a disabled hook leaves the trait
        // default and `cfg` alternatives for one field do not collide.
        let hints_fn = hints
            .iter()
            .filter(|(f, _, _)| *f == method.field)
            .map(|(_, f, cfgs)| {
                quote! {
                    #(#cfgs)*
                    fn hints(registry: &mut ::greem::HintRegistry<'_>) {
                        <Self>::#f(registry)
                    }
                }
            });
        let plan_fn = plans
            .iter()
            .filter(|(f, _, _)| *f == method.field)
            .map(|(_, f, cfgs)| {
                quote! {
                    #(#cfgs)*
                    fn plan(planning: &mut ::greem::Planning<'_, #marker_path, #context>) {
                        <Self>::#f(planning)
                    }
                }
            });
        let cfgs = &method.cfgs;
        impls.push(quote! {
            #(#cfgs)*
            impl #impl_generics ::greem::Resolver<#marker_path, #context> for #self_ty #where_clause {
                type Output<'obj> = #output_ty where Self: 'obj, #context: 'obj;

                fn resolve<'obj, 'call>(
                    parents: &'call [&'obj Self],
                    args: &'obj ::greem::Args<#marker_path>,
                    ctx: &'obj ::greem::Context<'obj, #context>,
                ) -> impl ::core::future::Future<Output = ::core::result::Result<::std::vec::Vec<Self::Output<'obj>>, ::greem::Error>> + ::core::marker::Send + 'call
                where
                    'obj: 'call,
                {
                    async move {
                        let _ = (args, ctx);
                        #body
                    }
                }

                #(#hints_fn)*
                #(#plan_fn)*
            }
        });
    }
    for (field, ident, _) in hints.iter().chain(plans.iter()) {
        if !methods.iter().any(|m| m.field == *field) {
            return Err(Error::new(
                ident.span(),
                format!("no resolver method for field `{field}` in this impl block"),
            ));
        }
    }
    Ok(quote! {
        #item
        #(#impls)*
    })
}

#[cfg(test)]
mod tests {
    use super::{ObjectArgs, expand_object};
    use quote::quote;

    #[test]
    fn raw_identifiers_name_the_field_without_the_prefix() {
        let args: ObjectArgs = syn::parse2(quote!(context = App)).unwrap();
        let item: syn::ItemImpl = syn::parse2(quote! {
            impl Query {
                fn r#type(&self) -> i32 { 1 }
                #[greem(name = "gen")]
                fn generator(&self) -> i32 { 2 }
            }
        })
        .unwrap();
        let code = expand_object(args, item).unwrap().to_string();
        assert!(code.contains(":: Query :: r#type"), "{code}");
        assert!(code.contains(":: Query :: r#gen"), "{code}");
        assert!(!code.contains("r#r#"), "{code}");
    }

    #[test]
    fn generated_impls_follow_the_method_cfg() {
        let args: ObjectArgs = syn::parse2(quote!(context = App)).unwrap();
        let item: syn::ItemImpl = syn::parse2(quote! {
            impl Query {
                #[cfg(feature = "x")]
                fn a(&self) -> i32 { 1 }
                #[cfg(not(feature = "x"))]
                fn a(&self) -> i32 { 2 }
                #[cfg_attr(all(), cfg(any()))]
                fn b(&self) -> i32 { 3 }
            }
        })
        .unwrap();
        let code = expand_object(args, item).unwrap().to_string();
        let flat: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains("#[cfg(feature=\"x\")]impl"), "{code}");
        assert!(flat.contains("#[cfg(not(feature=\"x\"))]impl"), "{code}");
        assert!(flat.contains("#[cfg_attr(all(),cfg(any()))]impl"), "{code}");
    }

    #[test]
    fn generated_hooks_follow_the_hook_cfg() {
        let args: ObjectArgs = syn::parse2(quote!(context = App)).unwrap();
        let item: syn::ItemImpl = syn::parse2(quote! {
            impl Query {
                fn value(&self) -> i32 { 1 }
                #[cfg(any())]
                #[greem(hints = "value")]
                fn value_hints(reg: &mut HintRegistry<'_>) {}
                #[greem(plan = "value")]
                #[cfg_attr(all(), cfg(any()))]
                fn value_plan(p: &mut Planning<'_, schema::Query::value, App>) {}
            }
        })
        .unwrap();
        let code = expand_object(args, item).unwrap().to_string();
        let flat: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains("#[cfg(any())]fnhints("), "{code}");
        assert!(
            flat.contains("#[cfg_attr(all(),cfg(any()))]fnplan("),
            "{code}"
        );
    }

    #[test]
    fn std_names_are_written_by_full_path() {
        let args: ObjectArgs = syn::parse2(quote!(context = App)).unwrap();
        let item: syn::ItemImpl = syn::parse2(quote! {
            impl Query {
                fn a(&self) -> i32 { 1 }
            }
        })
        .unwrap();
        let code = expand_object(args, item).unwrap().to_string();
        let flat: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains("+::core::marker::Send+'call"), "{code}");
        assert!(!flat.contains("+Send"), "{code}");
    }

    #[test]
    fn infallible_per_object_outputs_keep_their_type() {
        let args: ObjectArgs = syn::parse2(quote!(context = App)).unwrap();
        let item: syn::ItemImpl = syn::parse2(quote! {
            impl Query {
                fn values(&self) -> &[i32] { &[] }
                fn maybe(&self) -> Result<i32, greem::Error> { Ok(1) }
            }
        })
        .unwrap();
        let code = expand_object(args, item).unwrap().to_string();
        let flat: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains("typeOutput<'obj>=&'obj[i32]where"), "{code}");
        assert!(
            flat.contains("typeOutput<'obj>=::core::result::Result<i32,::greem::Error>where"),
            "{code}"
        );
    }
}
