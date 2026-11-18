use std::collections::BTreeSet;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Ident, Lit, LitStr, Meta, parse_macro_input};

/// 为具名字段结构体生成 descriptor getter 与 DescriptorBindingLayout。
/// 每个字段必须声明整数 binding；descriptor_type/count/stage/flags 可以省略，显式非法属性必须报错。
#[proc_macro_derive(DescriptorBinding, attributes(binding, descriptor_type, count, stage, flags))]
pub fn derive_descriptor_binding(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    BindingField::expand(&input).unwrap_or_else(|error| error.into_compile_error().into())
}

/// 一个字段的完整声明；解析和生成共用这一份配置，避免 getter 与 layout 投影漂移。
struct BindingField {
    name: Ident,
    method: Ident,
    binding: u32,
    descriptor_type: Expr,
    count: Expr,
    stage: Expr,
    flags: Expr,
}

impl BindingField {
    fn parse(field: &syn::Field) -> syn::Result<Self> {
        let name = field.ident.as_ref().expect("已检查具名字段").clone();
        let method_text = name.to_string();
        let mut method = syn::parse_str::<Ident>(method_text.trim_matches('_'))
            .map_err(|_| syn::Error::new_spanned(&name, "去掉首尾下划线后必须是有效方法名"))?;
        method.set_span(name.span());
        if matches!(method.to_string().trim_start_matches("r#"), "get_shader_bindings" | "get_vk_bindings") {
            return Err(syn::Error::new_spanned(&name, "字段方法名与 DescriptorBindingLayout 冲突"));
        }

        let binding = Self::value(&field.attrs, "binding")?
            .ok_or_else(|| syn::Error::new_spanned(&name, "descriptor 字段必须声明 #[binding = 整数]"))?;
        let binding = match binding {
            Expr::Lit(value) => match &value.lit {
                Lit::Int(value) => value.base10_parse::<u32>()?,
                _ => return Err(syn::Error::new_spanned(binding, "binding 必须是 u32 整数")),
            },
            _ => return Err(syn::Error::new_spanned(binding, "binding 必须是 u32 整数")),
        };
        let descriptor_type = match Self::value(&field.attrs, "descriptor_type")? {
            Some(value) => {
                let text = Self::string(value)?;
                let member = Self::member(text, text.value().trim())?;
                syn::parse_quote!(vk::DescriptorType::#member)
            }
            None => syn::parse_quote!(vk::DescriptorType::UNIFORM_BUFFER),
        };
        let count = match Self::value(&field.attrs, "count")? {
            Some(Expr::Lit(value)) if matches!(&value.lit, Lit::Str(_)) => {
                let Lit::Str(text) = &value.lit else { unreachable!() };
                text.parse::<Expr>()?
            }
            Some(value) => value.clone(),
            None => syn::parse_quote!(1),
        };
        let stage = Self::flags(
            Self::value(&field.attrs, "stage")?,
            syn::parse_quote!(vk::ShaderStageFlags),
            syn::parse_quote!(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
        )?;
        let flags = Self::flags(
            Self::value(&field.attrs, "flags")?,
            syn::parse_quote!(vk::DescriptorBindingFlags),
            syn::parse_quote!(vk::DescriptorBindingFlags::empty()),
        )?;
        Ok(Self {
            name,
            method,
            binding,
            descriptor_type,
            count,
            stage,
            flags,
        })
    }

    /// 默认值只表示属性缺省，不能掩盖重复属性或错误语法。
    fn value<'a>(attrs: &'a [Attribute], name: &str) -> syn::Result<Option<&'a Expr>> {
        let mut value = None;
        for attr in attrs.iter().filter(|attr| attr.path().is_ident(name)) {
            if value.is_some() {
                return Err(syn::Error::new_spanned(attr, format!("重复的 {name} 属性")));
            }
            let Meta::NameValue(meta) = &attr.meta else {
                return Err(syn::Error::new_spanned(attr, format!("{name} 必须使用 #[{name} = 值]")));
            };
            value = Some(&meta.value);
        }
        Ok(value)
    }

    fn string(value: &Expr) -> syn::Result<&LitStr> {
        if let Expr::Lit(value) = value
            && let Lit::Str(text) = &value.lit
        {
            return Ok(text);
        }
        Err(syn::Error::new_spanned(value, "属性值必须是字符串"))
    }

    fn member(text: &LitStr, name: &str) -> syn::Result<Ident> {
        let mut ident = syn::parse_str::<Ident>(name)
            .map_err(|_| syn::Error::new_spanned(text, "必须填写 Vulkan 常量名；flags 以 | 分隔"))?;
        ident.set_span(text.span());
        Ok(ident)
    }

    fn flags(value: Option<&Expr>, ty: syn::Path, default: Expr) -> syn::Result<Expr> {
        let Some(value) = value else { return Ok(default) };
        let text = Self::string(value)?;
        let members =
            text.value().split('|').map(|name| Self::member(text, name.trim())).collect::<syn::Result<Vec<_>>>()?;
        syn::parse2(quote!(#(#ty::#members)|*))
    }

    fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
        for attr in &input.attrs {
            if ["binding", "descriptor_type", "count", "stage", "flags"].iter().any(|name| attr.path().is_ident(name)) {
                return Err(syn::Error::new_spanned(attr, "descriptor 属性只能标注字段"));
            }
        }
        if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
            return Err(syn::Error::new_spanned(&input.generics, "descriptor layout 不支持泛型声明"));
        }
        let Data::Struct(data) = &input.data else {
            return Err(syn::Error::new_spanned(input, "DescriptorBinding 只支持具名字段结构体"));
        };
        let Fields::Named(fields) = &data.fields else {
            return Err(syn::Error::new_spanned(input, "DescriptorBinding 只支持具名字段结构体"));
        };
        let fields = fields.named.iter().map(Self::parse).collect::<syn::Result<Vec<_>>>()?;
        let mut bindings = BTreeSet::new();
        let mut methods = BTreeSet::new();
        for field in &fields {
            if !bindings.insert(field.binding) {
                return Err(syn::Error::new_spanned(&field.name, "同一 layout 的 binding 编号不能重复"));
            }
            if !methods.insert(field.method.to_string().trim_start_matches("r#").to_owned()) {
                return Err(syn::Error::new_spanned(&field.name, "去掉首尾下划线后的方法名重复"));
            }
        }

        let struct_name = &input.ident;
        let names = fields.iter().map(|field| &field.name).collect::<Vec<_>>();
        let methods = fields.iter().map(|field| &field.method).collect::<Vec<_>>();
        let bindings = fields.iter().map(|field| field.binding).collect::<Vec<_>>();
        let types = fields.iter().map(|field| &field.descriptor_type).collect::<Vec<_>>();
        let counts = fields.iter().map(|field| &field.count).collect::<Vec<_>>();
        let stages = fields.iter().map(|field| &field.stage).collect::<Vec<_>>();
        let flags = fields.iter().map(|field| &field.flags).collect::<Vec<_>>();
        Ok(quote! {
            impl #struct_name {
                #(
                    pub fn #methods() -> &'static truvis_descriptor_layout_trait::DescriptorBindingItem {
                        static ITEM: std::sync::OnceLock<truvis_descriptor_layout_trait::DescriptorBindingItem> = std::sync::OnceLock::new();
                        ITEM.get_or_init(|| truvis_descriptor_layout_trait::DescriptorBindingItem {
                            name: stringify!(#names).trim_matches('_'),
                            binding: #bindings,
                            descriptor_type: #types,
                            stage_flags: #stages,
                            count: #counts,
                            flags: #flags,
                        })
                    }
                )*
            }
            impl truvis_descriptor_layout_trait::DescriptorBindingLayout for #struct_name {
                fn get_shader_bindings() -> Vec<truvis_descriptor_layout_trait::DescriptorBindingItem> {
                    vec![#(*Self::#methods()),*]
                }
            }
        }.into())
    }
}
